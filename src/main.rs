mod cache_key;
mod cache_model;
mod catalog;
mod config;
mod db;
mod onshape;
mod parameters;
mod storage;

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    env,
    io::{Cursor, Read},
    sync::Arc,
    time::Duration,
};

use anyhow::Context;
use axum::{
    Form, Json, Router,
    extract::{Path, State},
    http::{StatusCode, header::CONTENT_TYPE},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tower_http::trace::TraceLayer;
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

use crate::parameters::{
    ParameterKind, ParameterSchema, ParameterVisibilityCondition, SCHEMA_VERSION,
    ValidatedConfiguration, apply_overrides, normalize_configuration, validate_values,
};
use crate::{
    cache_model::{EncodedConfigurationIdentity, ResolvedOnshapeSourceIdentity},
    catalog::Catalog,
    config::Config,
    db::{ArtifactUpsert, Database},
    onshape::OnshapeClient,
    storage::StorageClient,
};

const EXPORTER_VERSION: &str = env!("CARGO_PKG_VERSION");
const MANIFEST_SCHEMA_VERSION: u32 = 1;
const PREVIEW_OPTIONS_VERSION: &str = "mesh-grouped-v2";
const DOWNLOAD_OPTIONS_VERSION: &str = "default-v1";
const CONFIG_HASH_JOB_VERSION: u32 = 1;
const RETRY_BACKOFF_BASE_SECONDS: i64 = 30;
const RETRY_BACKOFF_CAP_SECONDS: i64 = 5 * 60;
const ALLOW_PARTIAL_MULTI_GLTF_PREVIEW_FALLBACK: bool = false;

#[derive(Clone)]
struct AppState {
    catalog: Arc<Catalog>,
    db: Database,
    onshape: OnshapeClient,
    storage: StorageClient,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum JobPayload {
    ParameterRefresh {
        model_slug: String,
    },
    PreviewGlb {
        model_slug: String,
        #[serde(default)]
        config_hash: String,
        #[serde(default)]
        config_hash_version: Option<u32>,
        values: HashMap<String, String>,
    },
    DownloadExport {
        model_slug: String,
        #[serde(default)]
        config_hash: String,
        #[serde(default)]
        config_hash_version: Option<u32>,
        values: HashMap<String, String>,
        format: catalog::DownloadFormat,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutputFormat {
    Text,
    Json,
}

#[derive(Debug, Clone)]
struct SelectedParameterSet {
    slug: String,
    label: String,
    validated: ValidatedConfiguration,
}

#[derive(Debug, Clone, Copy)]
struct PruneOptions {
    older_than_days: i64,
    dry_run: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ManifestOptions {
    rewrite: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FailureRetrySelector<'a> {
    All,
    Kind(&'a str),
    WorkKey(&'a str),
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ArtifactManifest {
    manifest_schema_version: u32,
    group_id: String,
    model_slug: String,
    onshape: ManifestOnshapeSource,
    configuration: ManifestConfiguration,
    outputs: BTreeMap<String, ManifestOutput>,
    created_at: Option<String>,
    exporter_version: &'static str,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ManifestOnshapeSource {
    document_id: String,
    version_id: String,
    element_id: String,
    element_kind: String,
    link_document_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ManifestConfiguration {
    hash: String,
    values: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ManifestOutput {
    artifact_key: String,
    object_key: String,
    public_url: Option<String>,
    status: String,
    content_type: String,
    size_bytes: Option<i64>,
    sha256: Option<String>,
    job_id: Option<String>,
    source_hash: Option<String>,
    options_hash: Option<String>,
    schema_version: Option<i64>,
    created_at: String,
    superseded_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ArtifactStatusResponse {
    artifact_key: String,
    status: String,
    message: String,
    public_url: Option<String>,
    object_key: Option<String>,
    content_type: Option<String>,
    size_bytes: Option<i64>,
    sha256: Option<String>,
    job_id: Option<String>,
    source_hash: Option<String>,
    config_hash: Option<String>,
    options_hash: Option<String>,
    schema_version: Option<i64>,
    attempt: Option<i64>,
    max_attempts: Option<i64>,
    next_retry_at: Option<String>,
    error_summary: Option<String>,
    updated_at: Option<String>,
}

#[derive(Debug)]
struct PreviewArtifact {
    object_key: String,
    content_type: &'static str,
    bytes: Vec<u8>,
    sidecars: Vec<PreviewAsset>,
}

#[derive(Debug)]
struct PreviewAsset {
    object_key: String,
    content_type: &'static str,
    bytes: Vec<u8>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();

    let config = Config::from_env()?;
    let args = env::args().skip(1).collect::<Vec<_>>();

    match args.first().map(String::as_str) {
        None | Some("serve") => serve(config).await,
        Some("worker") => run_worker(config).await,
        Some(command) => run_cli(config, command, &args[1..]).await,
    }
}

async fn serve(config: Config) -> anyhow::Result<()> {
    let worker_enabled = config.worker_enabled;
    let worker_concurrency = config.worker_concurrency;
    let bind_addr = config.bind_addr;
    let rebuild_interval = config.rebuild_interval;
    let state = build_state(config).await?;

    if worker_enabled {
        tokio::spawn(background_runtime(
            state.clone(),
            rebuild_interval,
            worker_concurrency,
        ));
    } else {
        tracing::info!("background worker disabled for serve process");
    }

    let listener = tokio::net::TcpListener::bind(bind_addr)
        .await
        .context("binding listener")?;
    tracing::info!(address = %listener.local_addr()?, "listening");

    axum::serve(listener, app(state))
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("serving app")
}

async fn run_worker(config: Config) -> anyhow::Result<()> {
    let rebuild_interval = config.rebuild_interval;
    let worker_concurrency = config.worker_concurrency;
    let state = build_state(config).await?;
    tracing::info!(worker_concurrency, "starting worker-only runtime");

    tokio::select! {
        () = background_runtime(state, rebuild_interval, worker_concurrency) => {},
        () = shutdown_signal() => {
            tracing::info!("worker shutdown requested");
        },
    }

    Ok(())
}

async fn run_cli(config: Config, command: &str, args: &[String]) -> anyhow::Result<()> {
    match (command, args) {
        ("catalog", [subcommand]) if subcommand == "validate" => {
            let catalog = Catalog::load(&config.catalog_path).context("loading catalog")?;
            println!("catalog ok: {} models", catalog.models().len());
            Ok(())
        }
        ("ops", [subcommand]) if subcommand == "check" => run_ops_check(config).await,
        ("ops", [subcommand, destination]) if subcommand == "backup" => {
            let db = Database::connect(&config.database_url)
                .await
                .context("connecting to database")?;
            let destination = std::path::Path::new(destination);
            db.backup_to_path(destination).await?;
            println!("database backup written to {}", destination.display());
            Ok(())
        }
        ("parameters", [subcommand, selector]) if subcommand == "refresh" => {
            let state = cli_state(config).await?;
            for model in selected_models(&state.catalog, selector)? {
                refresh_parameters(&state, model).await?;
                println!("refreshed parameters for {}", model.slug);
            }
            Ok(())
        }
        ("previews", [subcommand, selector, parameter_selector @ ..])
            if subcommand == "generate" =>
        {
            let parameter_selector = optional_parameter_selector(parameter_selector)?;
            let state = cli_state(config).await?;
            for model in selected_models(&state.catalog, selector)? {
                for parameter_set in
                    selected_parameter_sets(&state, model, parameter_selector).await?
                {
                    let object_key =
                        generate_preview_for_values(&state, model, &parameter_set.validated)
                            .await?;
                    println!(
                        "preview ready for {} [{} - {}]: {object_key}",
                        model.slug, parameter_set.slug, parameter_set.label
                    );
                }
            }
            Ok(())
        }
        ("exports", [subcommand, selector, format, parameter_selector @ ..])
            if subcommand == "generate" =>
        {
            let parameter_selector = optional_parameter_selector(parameter_selector)?;
            let state = cli_state(config).await?;
            for model in selected_models(&state.catalog, selector)? {
                let formats = selected_formats(model, format)?;
                for parameter_set in
                    selected_parameter_sets(&state, model, parameter_selector).await?
                {
                    for format in &formats {
                        match generate_download_for_values(
                            &state,
                            model,
                            &parameter_set.validated,
                            *format,
                        )
                        .await?
                        {
                            Some(object_key) => println!(
                                "{} export ready for {} [{} - {}]: {object_key}",
                                format.label(),
                                model.slug,
                                parameter_set.slug,
                                parameter_set.label
                            ),
                            None => unreachable!("values were already validated"),
                        }
                    }
                }
            }
            Ok(())
        }
        ("failures", [subcommand, output_args @ ..]) if subcommand == "list" => {
            let output_format = optional_output_format(output_args)?;
            let state = cli_state(config).await?;
            let jobs = state.db.failed_jobs(100).await?;
            print_jobs(jobs, output_format, "no failed jobs")?;
            Ok(())
        }
        ("jobs", [subcommand, output_args @ ..]) if subcommand == "list" => {
            let output_format = optional_output_format(output_args)?;
            let state = cli_state(config).await?;
            let jobs = state.db.jobs(100).await?;
            print_jobs(jobs, output_format, "no jobs")?;
            Ok(())
        }
        ("failures", [subcommand, retry_args @ ..]) if subcommand == "retry" => {
            let selector = optional_failure_retry_selector(retry_args)?;
            let state = cli_state(config).await?;
            match selector {
                FailureRetrySelector::All => {
                    let count = state.db.retry_failed_jobs().await?;
                    println!("marked {count} failed jobs for retry");
                }
                FailureRetrySelector::Kind(job_kind) => {
                    let count = state.db.retry_failed_jobs_by_kind(job_kind).await?;
                    println!("marked {count} {job_kind} failed job(s) for retry");
                }
                FailureRetrySelector::WorkKey(work_key) => {
                    if state.db.retry_failed_job(work_key).await? {
                        println!("marked failed job {work_key} for retry");
                    } else {
                        println!("failed job not found or not retryable: {work_key}");
                    }
                }
            }
            Ok(())
        }
        ("artifacts", [subcommand, selector, output_args @ ..]) if subcommand == "list" => {
            let output_format = optional_output_format(output_args)?;
            let state = cli_state(config).await?;
            let mut all_artifacts = Vec::new();
            for model in selected_models(&state.catalog, selector)? {
                let artifacts = state.db.artifacts_for_model(&model.slug).await?;
                match output_format {
                    OutputFormat::Json => all_artifacts.extend(artifacts),
                    OutputFormat::Text if artifacts.is_empty() => {
                        println!("no artifacts for {}", model.slug);
                    }
                    OutputFormat::Text => {
                        for artifact in artifacts {
                            println!(
                                "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                                artifact.artifact_key,
                                artifact.model_slug,
                                artifact.config_hash,
                                artifact.output_kind,
                                artifact.status,
                                artifact.content_type,
                                artifact.byte_len.unwrap_or_default(),
                                artifact.sha256.as_deref().unwrap_or(""),
                                artifact.created_at,
                                artifact.object_key
                            );
                        }
                    }
                }
            }
            if output_format == OutputFormat::Json {
                println!("{}", serde_json::to_string_pretty(&all_artifacts)?);
            }
            Ok(())
        }
        ("artifacts", [subcommand, artifact_key]) if subcommand == "invalidate" => {
            let state = cli_state(config).await?;
            let Some(artifact) = state.db.artifact(artifact_key).await? else {
                println!("artifact not found: {artifact_key}");
                return Ok(());
            };

            supersede_artifact_and_rewrite_manifest(&state, &artifact).await?;
            println!(
                "invalidated artifact {artifact_key} and marked {} superseded",
                artifact.object_key
            );
            Ok(())
        }
        ("artifacts", [subcommand, slug, config_hash, manifest_args @ ..])
            if subcommand == "manifest" =>
        {
            let options = parse_manifest_options(manifest_args)?;
            let state = cli_state(config).await?;
            let model = state
                .catalog
                .find(slug)
                .ok_or_else(|| anyhow::anyhow!("unknown model slug: {slug}"))?;
            let artifacts = state
                .db
                .artifacts_for_configuration(&model.slug, config_hash)
                .await?;
            let source_hash = artifacts
                .first()
                .and_then(|artifact| artifact.source_hash.clone())
                .unwrap_or(resolve_source_hash(&state, model).await?);
            let manifest = build_manifest(
                &source_hash,
                model,
                config_hash,
                None,
                &artifacts,
                state.storage.public_base_url(),
            );

            if options.rewrite {
                let object_key = manifest_object_key(&source_hash, config_hash);
                state.storage.put_json(&object_key, &manifest).await?;
                eprintln!("rewrote manifest {object_key}");
            }

            println!("{}", serde_json::to_string_pretty(&manifest)?);
            Ok(())
        }
        ("artifacts", [subcommand, selector, prune_args @ ..]) if subcommand == "prune" => {
            let options = parse_prune_options(prune_args)?;
            let state = cli_state(config).await?;
            let mut pruned = 0usize;

            for model in selected_models(&state.catalog, selector)? {
                let artifacts = state
                    .db
                    .artifacts_older_than_days(&model.slug, options.older_than_days)
                    .await?;
                if artifacts.is_empty() {
                    println!(
                        "no artifacts older than {} days for {}",
                        options.older_than_days, model.slug
                    );
                    continue;
                }

                for artifact in artifacts {
                    println!(
                        "{} {} {} {} {}",
                        if options.dry_run {
                            "would supersede"
                        } else {
                            "superseding"
                        },
                        artifact.artifact_key,
                        artifact.created_at,
                        artifact.byte_len.unwrap_or_default(),
                        artifact.object_key,
                    );
                    if !options.dry_run {
                        supersede_artifact_and_rewrite_manifest(&state, &artifact).await?;
                    }
                    pruned += 1;
                }
            }

            println!(
                "{} {pruned} artifact(s) older than {} days",
                if options.dry_run {
                    "matched"
                } else {
                    "superseded"
                },
                options.older_than_days
            );
            Ok(())
        }
        _ => {
            print_usage();
            anyhow::bail!("unknown command")
        }
    }
}

fn print_jobs(
    jobs: Vec<db::JobRecord>,
    output_format: OutputFormat,
    empty_message: &str,
) -> anyhow::Result<()> {
    match output_format {
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&jobs)?),
        OutputFormat::Text if jobs.is_empty() => println!("{empty_message}"),
        OutputFormat::Text => {
            for job in jobs {
                println!(
                    "{}\t{}\t{}\tattempt={}/{}\tnext_retry={}\tcreated={}\tupdated={}\t{}",
                    job.work_key,
                    job.job_kind,
                    job.status,
                    job.attempt,
                    job.max_attempts,
                    job.next_retry_at.as_deref().unwrap_or(""),
                    job.created_at,
                    job.updated_at,
                    job.error_summary.unwrap_or_default()
                );
            }
        }
    }
    Ok(())
}

fn optional_output_format(args: &[String]) -> anyhow::Result<OutputFormat> {
    match args {
        [] => Ok(OutputFormat::Text),
        [flag] if flag == "--json" => Ok(OutputFormat::Json),
        [flag] => anyhow::bail!("unknown output option: {flag}"),
        _ => anyhow::bail!("expected at most one output option"),
    }
}

async fn run_ops_check(config: Config) -> anyhow::Result<()> {
    let mut failures = Vec::new();

    match Catalog::load(&config.catalog_path) {
        Ok(catalog) => println!("catalog ok: {} models", catalog.models().len()),
        Err(error) => failures.push(format!("catalog load failed: {error:#}")),
    }

    match Database::connect(&config.database_url).await {
        Ok(db) => match db.ping().await {
            Ok(()) => println!("database ok: {}", config.database_url),
            Err(error) => failures.push(format!("database ping failed: {error:#}")),
        },
        Err(error) => failures.push(format!("database connect failed: {error:#}")),
    }

    match StorageClient::new(config.storage.clone()).await {
        Ok(storage) => println!("storage client ok: bucket {}", storage.bucket()),
        Err(error) => failures.push(format!("storage client failed: {error:#}")),
    }
    if config.storage.access_key_id.is_none() || config.storage.secret_access_key.is_none() {
        failures.push(
            "storage credentials are incomplete; set AWS_ACCESS_KEY_ID and AWS_SECRET_ACCESS_KEY"
                .to_owned(),
        );
    }
    if config.storage.public_base_url.is_none() {
        failures.push(
            "TIGRIS_PUBLIC_BASE_URL is not set; generated artifact URLs will not be public"
                .to_owned(),
        );
    }

    match OnshapeClient::new(config.onshape.clone()) {
        Ok(onshape) => println!("onshape client ok: {}", onshape.base_url()),
        Err(error) => failures.push(format!("onshape client failed: {error:#}")),
    }
    if config.onshape.access_key.is_none() || config.onshape.secret_key.is_none() {
        failures.push(
            "Onshape credentials are incomplete; set ONSHAPE_ACCESS_KEY and ONSHAPE_SECRET_KEY"
                .to_owned(),
        );
    }

    if failures.is_empty() {
        println!("ops check ok");
        Ok(())
    } else {
        for failure in &failures {
            eprintln!("ops check failed: {failure}");
        }
        anyhow::bail!("ops check failed with {} issue(s)", failures.len())
    }
}

fn optional_failure_retry_selector(args: &[String]) -> anyhow::Result<FailureRetrySelector<'_>> {
    match args {
        [] => Ok(FailureRetrySelector::All),
        [flag] if flag == "--all" => Ok(FailureRetrySelector::All),
        [flag, job_kind] if flag == "--kind" => Ok(FailureRetrySelector::Kind(job_kind)),
        [work_key] if work_key.starts_with("--") => {
            anyhow::bail!("unknown failures retry option: {work_key}")
        }
        [work_key] => Ok(FailureRetrySelector::WorkKey(work_key)),
        _ => anyhow::bail!("expected at most one failure retry selector, or --kind <job-kind>"),
    }
}

fn optional_parameter_selector(args: &[String]) -> anyhow::Result<Option<&str>> {
    match args {
        [] => Ok(None),
        [selector] => Ok(Some(selector.as_str())),
        _ => anyhow::bail!("expected at most one parameter set selector"),
    }
}

fn parse_prune_options(args: &[String]) -> anyhow::Result<PruneOptions> {
    let mut older_than_days = None;
    let mut dry_run = false;
    let mut args = args.iter();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--older-than-days" => {
                let value = args
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("--older-than-days requires a value"))?;
                let days = value
                    .parse::<i64>()
                    .with_context(|| format!("invalid --older-than-days value: {value}"))?;
                anyhow::ensure!(days > 0, "--older-than-days must be greater than zero");
                older_than_days = Some(days);
            }
            "--dry-run" => dry_run = true,
            _ => anyhow::bail!("unknown prune option: {arg}"),
        }
    }

    Ok(PruneOptions {
        older_than_days: older_than_days
            .ok_or_else(|| anyhow::anyhow!("--older-than-days is required"))?,
        dry_run,
    })
}

fn parse_manifest_options(args: &[String]) -> anyhow::Result<ManifestOptions> {
    let mut rewrite = false;

    for arg in args {
        match arg.as_str() {
            "--rewrite" => rewrite = true,
            _ => anyhow::bail!("unknown manifest option: {arg}"),
        }
    }

    Ok(ManifestOptions { rewrite })
}

async fn cli_state(config: Config) -> anyhow::Result<AppState> {
    build_state(config).await
}

async fn build_state(config: Config) -> anyhow::Result<AppState> {
    let catalog = Arc::new(Catalog::load(&config.catalog_path).context("loading catalog")?);
    let db = Database::connect(&config.database_url)
        .await
        .context("connecting to database")?;
    let storage = StorageClient::new(config.storage.clone()).await?;
    let onshape = OnshapeClient::new(config.onshape.clone())?;

    Ok(AppState {
        catalog,
        db,
        onshape,
        storage,
    })
}

fn selected_models<'a>(
    catalog: &'a Catalog,
    selector: &str,
) -> anyhow::Result<Vec<&'a catalog::Model>> {
    if selector == "--all" {
        return Ok(catalog.models().iter().collect());
    }

    catalog
        .find(selector)
        .map(|model| vec![model])
        .ok_or_else(|| anyhow::anyhow!("unknown model slug: {selector}"))
}

fn selected_formats(
    model: &catalog::Model,
    selector: &str,
) -> anyhow::Result<Vec<catalog::DownloadFormat>> {
    if selector == "--all" {
        return Ok(model.exports.downloads.clone());
    }

    let format = catalog::DownloadFormat::from_slug(selector)
        .ok_or_else(|| anyhow::anyhow!("unknown export format: {selector}"))?;
    anyhow::ensure!(
        model.exports.downloads.contains(&format),
        "{} does not expose {} downloads",
        model.slug,
        format.label()
    );
    Ok(vec![format])
}

async fn generate_preview_for_values(
    state: &AppState,
    model: &catalog::Model,
    validated: &ValidatedConfiguration,
) -> anyhow::Result<String> {
    let source_hash = resolve_source_hash(state, model).await?;
    let config_hash = persist_configuration_selection(state, &source_hash, validated).await?;
    let artifact_key = preview_artifact_key(&source_hash, model, &config_hash);

    if let Some(record) = state.db.artifact(&artifact_key).await? {
        return Ok(record.object_key);
    }

    refresh_preview(
        state,
        model,
        &source_hash,
        &validated.values,
        &config_hash,
        &artifact_key,
        None,
    )
    .await
}

async fn enqueue_parameter_refresh(
    state: &AppState,
    model: &catalog::Model,
) -> anyhow::Result<bool> {
    enqueue_parameter_refresh_with_force(state, model, false).await
}

async fn force_enqueue_parameter_refresh(
    state: &AppState,
    model: &catalog::Model,
) -> anyhow::Result<bool> {
    enqueue_parameter_refresh_with_force(state, model, true).await
}

async fn enqueue_parameter_refresh_with_force(
    state: &AppState,
    model: &catalog::Model,
    force: bool,
) -> anyhow::Result<bool> {
    let source_hash = resolve_source_hash(state, model).await?;
    let payload = JobPayload::ParameterRefresh {
        model_slug: model.slug.clone(),
    };
    enqueue_job(
        state,
        &parameter_refresh_work_key(&source_hash),
        "parameter_refresh",
        &payload,
        force,
    )
    .await
}

async fn enqueue_preview(
    state: &AppState,
    model: &catalog::Model,
    validated: &ValidatedConfiguration,
) -> anyhow::Result<bool> {
    let source_hash = resolve_source_hash(state, model).await?;
    let config_hash = persist_configuration_selection(state, &source_hash, validated).await?;
    let payload = JobPayload::PreviewGlb {
        model_slug: model.slug.clone(),
        config_hash: config_hash.clone(),
        config_hash_version: Some(CONFIG_HASH_JOB_VERSION),
        values: validated.values.clone(),
    };
    enqueue_job(
        state,
        &preview_work_key(&source_hash, model, &config_hash),
        "preview_export",
        &payload,
        false,
    )
    .await
}

async fn enqueue_download(
    state: &AppState,
    model: &catalog::Model,
    validated: &ValidatedConfiguration,
    format: catalog::DownloadFormat,
) -> anyhow::Result<bool> {
    let source_hash = resolve_source_hash(state, model).await?;
    let config_hash = persist_configuration_selection(state, &source_hash, validated).await?;
    let payload = JobPayload::DownloadExport {
        model_slug: model.slug.clone(),
        config_hash: config_hash.clone(),
        config_hash_version: Some(CONFIG_HASH_JOB_VERSION),
        values: validated.values.clone(),
        format,
    };
    enqueue_job(
        state,
        &download_work_key(&source_hash, model, &config_hash, format),
        "download_export",
        &payload,
        false,
    )
    .await
}

async fn enqueue_job(
    state: &AppState,
    work_key: &str,
    job_kind: &str,
    payload: &JobPayload,
    force: bool,
) -> anyhow::Result<bool> {
    let payload_json = serde_json::to_string(payload)?;
    if force {
        Ok(state
            .db
            .force_enqueue_job(work_key, job_kind, &payload_json)
            .await?)
    } else {
        Ok(state
            .db
            .enqueue_job(work_key, job_kind, &payload_json)
            .await?)
    }
}

async fn generate_download_for_values(
    state: &AppState,
    model: &catalog::Model,
    validated: &ValidatedConfiguration,
    format: catalog::DownloadFormat,
) -> anyhow::Result<Option<String>> {
    let source_hash = resolve_source_hash(state, model).await?;
    let config_hash = persist_configuration_selection(state, &source_hash, validated).await?;
    let artifact_key = download_artifact_key(&source_hash, model, &config_hash, format);

    if let Some(record) = state.db.artifact(&artifact_key).await? {
        return Ok(Some(record.object_key));
    }

    refresh_download(
        state,
        model,
        &source_hash,
        &validated.values,
        &config_hash,
        &artifact_key,
        format,
        None,
    )
    .await
    .map(Some)
}

async fn selected_parameter_sets(
    state: &AppState,
    model: &catalog::Model,
    selector: Option<&str>,
) -> anyhow::Result<Vec<SelectedParameterSet>> {
    let schema = refresh_parameters(state, model).await?;
    let requested = selector.unwrap_or("default");
    let mut sets = Vec::new();

    if requested == "default" || requested == "--all-parameter-sets" {
        sets.push(validated_parameter_set(
            &schema,
            model,
            "default".to_owned(),
            "Default".to_owned(),
            &HashMap::new(),
        )?);
    }

    for preset in &model.parameter_presets {
        if requested == "--all-parameter-sets" || requested == preset.slug {
            sets.push(validated_parameter_set(
                &schema,
                model,
                preset.slug.clone(),
                preset.name.clone(),
                &preset.values,
            )?);
        }
    }

    if sets.is_empty() {
        anyhow::bail!("unknown parameter set for {}: {requested}", model.slug);
    }

    Ok(sets)
}

fn validated_parameter_set(
    schema: &ParameterSchema,
    model: &catalog::Model,
    slug: String,
    label: String,
    submitted: &HashMap<String, String>,
) -> anyhow::Result<SelectedParameterSet> {
    let validated = validate_values(schema, submitted, model.parameter_policy.allow_unknown)
        .map_err(|errors| {
            anyhow::anyhow!(
                "{} [{}] parameters are invalid: {}",
                model.slug,
                slug,
                errors.join(", ")
            )
        })?;

    Ok(SelectedParameterSet {
        slug,
        label,
        validated,
    })
}

fn print_usage() {
    eprintln!(
        "usage:\n  onshape-export [serve]\n  onshape-export worker\n  onshape-export catalog validate\n  onshape-export ops check\n  onshape-export ops backup <destination.db>\n  onshape-export parameters refresh <slug|--all>\n  onshape-export previews generate <slug|--all> [default|preset-slug|--all-parameter-sets]\n  onshape-export exports generate <slug|--all> <step|stl|3mf|--all> [default|preset-slug|--all-parameter-sets]\n  onshape-export jobs list [--json]\n  onshape-export failures list [--json]\n  onshape-export failures retry [--all|<work-key>|--kind <job-kind>]\n  onshape-export artifacts list <slug|--all> [--json]\n  onshape-export artifacts manifest <slug> <config-hash> [--rewrite]\n  onshape-export artifacts invalidate <artifact-key>\n  onshape-export artifacts prune <slug|--all> --older-than-days <days> [--dry-run]"
    );
}

fn app(state: AppState) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/healthz", get(healthz))
        .route("/metrics", get(metrics))
        .route(
            "/models/{slug}",
            get(model_page).post(validate_model_config),
        )
        .route("/models/{slug}/preview", post(generate_preview))
        .route(
            "/models/{slug}/preview/{config_hash}/status",
            get(preview_status),
        )
        .route("/models/{slug}/exports/{format}", post(generate_download))
        .route(
            "/models/{slug}/exports/{format}/{config_hash}/status",
            get(download_status),
        )
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

async fn healthz(State(state): State<AppState>) -> Result<&'static str, AppError> {
    state.db.ping().await?;
    let _ = state.storage.bucket();
    let _ = state.storage.public_base_url();
    let _ = state.storage.client();
    let _ = state.onshape.base_url();
    let _ = state.onshape.has_credentials();
    let _ = state.onshape.client();
    Ok("ok\n")
}

async fn metrics(State(state): State<AppState>) -> Result<impl IntoResponse, AppError> {
    let job_metrics = state.db.job_metrics().await?;
    let artifact_metrics = state.db.artifact_metrics().await?;
    let body = render_metrics(
        state.catalog.models().len(),
        &job_metrics,
        &artifact_metrics,
    );

    Ok(([(CONTENT_TYPE, "text/plain; version=0.0.4")], body))
}

async fn index(State(state): State<AppState>) -> Html<String> {
    let models = state
        .catalog
        .models()
        .iter()
        .filter(|model| model.published)
        .map(|model| {
            format!(
                r#"<li><a href="/models/{slug}">{name}</a><p>{description}</p></li>"#,
                slug = escape_html(&model.slug),
                name = escape_html(&model.name),
                description = escape_html(&model.description),
            )
        })
        .collect::<String>();

    Html(format!(
        r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>Onshape Export</title>
</head>
<body>
  <main>
    <h1>Onshape Export</h1>
    <p>Curated configurable models will appear here as catalog entries are added.</p>
    <ul>{models}</ul>
  </main>
</body>
</html>"#
    ))
}

async fn model_page(
    State(state): State<AppState>,
    Path(slug): Path<String>,
) -> Result<Html<String>, AppError> {
    let model = published_model(&state.catalog, &slug).ok_or(AppError::NotFound)?;
    let parameters = load_or_refresh_parameters(&state, model).await?;
    let parameter_controls = parameters
        .as_ref()
        .map(render_parameter_controls)
        .unwrap_or_else(|| {
            "<p>Parameter metadata refresh is already running. Reload this page shortly.</p>"
                .to_owned()
        });
    let preview = match parameters.as_ref() {
        Some(parameters) => render_cached_preview(&state, model, parameters).await?,
        None => "<p>Preview unavailable until parameter metadata is ready.</p>".to_owned(),
    };
    let downloads = match parameters.as_ref() {
        Some(parameters) => render_cached_downloads(&state, model, parameters).await?,
        None => "<p>Downloads unavailable until parameter metadata is ready.</p>".to_owned(),
    };

    Ok(render_model_html(
        model,
        &parameter_controls,
        &preview,
        &downloads,
    ))
}

fn published_model<'a>(catalog: &'a Catalog, slug: &str) -> Option<&'a catalog::Model> {
    catalog.find(slug).filter(|model| model.published)
}

fn render_model_html(
    model: &catalog::Model,
    parameter_controls: &str,
    preview: &str,
    downloads: &str,
) -> Html<String> {
    Html(format!(
        r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <script type="module" src="https://ajax.googleapis.com/ajax/libs/model-viewer/4.0.0/model-viewer.min.js"></script>
  <title>{name} - Onshape Export</title>
  <style>
    body {{
      margin: 0;
      font-family: system-ui, sans-serif;
      line-height: 1.4;
    }}
    main {{
      padding: 1rem;
    }}
    .model-layout {{
      display: grid;
      grid-template-columns: minmax(18rem, 24rem) minmax(0, 1fr);
      gap: 1.5rem;
      align-items: start;
    }}
    .parameters-panel {{
      position: sticky;
      top: 1rem;
      max-height: calc(100vh - 2rem);
      overflow: auto;
      padding-right: 0.25rem;
    }}
    .actions {{
      display: flex;
      flex-wrap: wrap;
      gap: 0.5rem;
      margin-top: 1rem;
    }}
    .output-panel {{
      min-width: 0;
    }}
    .preview-section model-viewer {{
      width: min(100%, 52rem) !important;
      height: min(70vh, 42rem) !important;
    }}
    input, select, textarea, button {{
      font: inherit;
    }}
    input, select, textarea {{
      box-sizing: border-box;
      max-width: 100%;
    }}
    .parameter-control {{
      display: grid;
      grid-template-columns: minmax(8rem, 42%) minmax(0, 1fr);
      gap: 0.75rem;
      align-items: start;
      margin: 0.75rem 0;
    }}
    .parameter-control[hidden] {{
      display: none;
    }}
    .parameter-label {{
      font-weight: 600;
      text-align: right;
    }}
    .parameter-value {{
      min-width: 0;
    }}
    .parameter-value input:not([type="checkbox"]):not([type="hidden"]),
    .parameter-value select,
    .parameter-value textarea {{
      width: 100%;
    }}
    .parameter-value small {{
      display: block;
      margin-top: 0.25rem;
      color: #555;
    }}
    @media (max-width: 800px) {{
      .model-layout {{
        grid-template-columns: 1fr;
      }}
      .parameters-panel {{
        position: static;
        max-height: none;
      }}
    }}
    @media (max-width: 520px) {{
      .parameter-control {{
        grid-template-columns: 1fr;
        gap: 0.25rem;
      }}
      .parameter-label {{
        text-align: left;
      }}
    }}
  </style>
</head>
<body>
  <main>
    <p><a href="/">Back to catalog</a></p>
    <h1>{name}</h1>
    <p>{description}</p>
    <div class="model-layout">
      <section class="parameters-panel" aria-labelledby="parameters-heading">
        <h2 id="parameters-heading">Parameters</h2>
        <form method="post">
          {parameter_controls}
          <div class="actions">
            <button type="submit">Validate Parameters</button>
            <button type="submit" formaction="/models/{slug}/preview">Generate Preview</button>
            {download_buttons}
          </div>
        </form>
      </section>
      <div class="output-panel">
        <section class="preview-section">
          <h2>Preview</h2>
          {preview}
        </section>
        <section>
          <h2>Downloads</h2>
          {downloads}
        </section>
      </div>
    </div>
  </main>
  <script>
(() => {{
  const isNearWhiteColor = (color) => {{
    if (!color || color.length < 3) {{
      return false;
    }}
    const min = Math.min(color[0], color[1], color[2]);
    const max = Math.max(color[0], color[1], color[2]);
    return min >= 0.85 && max - min <= 0.08;
  }};

  window.onshapeExportConfigurePreviewViewer = (viewer) => {{
    viewer.setAttribute("environment-image", "neutral");
    viewer.setAttribute("exposure", "0.7");
    viewer.setAttribute("shadow-intensity", "0.85");
    viewer.setAttribute("shadow-softness", "0.6");
    viewer.style.background = "linear-gradient(#3b3f45, #25282d)";

    const applyMaterialPreset = () => {{
      for (const material of viewer.model?.materials ?? []) {{
        const pbr = material.pbrMetallicRoughness;
        const color = pbr?.baseColorFactor;
        if (!isNearWhiteColor(color) || typeof pbr.setBaseColorFactor !== "function") {{
          continue;
        }}
        pbr.setBaseColorFactor([0.48, 0.50, 0.52, color[3] ?? 1]);
        pbr.setRoughnessFactor?.(0.74);
      }}
    }};

    if (viewer.model) {{
      applyMaterialPreset();
    }} else {{
      viewer.addEventListener("load", applyMaterialPreset, {{ once: true }});
    }}
  }};

  const configurePreviewViewers = (root) => {{
    for (const viewer of root.querySelectorAll("model-viewer")) {{
      window.onshapeExportConfigurePreviewViewer(viewer);
    }}
  }};

  const normalizeParameterValue = (value) => {{
    if (value === "on" || value === "1") {{
      return "true";
    }}
    if (value === "0") {{
      return "false";
    }}
    return value;
  }};

  const parameterValue = (form, parameterId) => {{
    const controls = Array.from(form.elements).filter((control) => control.name === parameterId);
    for (const control of controls) {{
      if (control instanceof HTMLInputElement && control.type === "checkbox") {{
        if (control.checked) {{
          return control.value || "on";
        }}
        continue;
      }}
      if (control instanceof HTMLInputElement && control.type === "radio") {{
        if (control.checked) {{
          return control.value;
        }}
        continue;
      }}
    }}

    const control = controls.find((control) =>
      !(control instanceof HTMLInputElement) ||
      (control.type !== "checkbox" && control.type !== "radio")
    );
    return control?.value;
  }};

  const evaluateVisibilityCondition = (condition, form) => {{
    if (!condition || typeof condition !== "object") {{
      return true;
    }}

    if (condition.kind === "all") {{
      const conditions = Array.isArray(condition.conditions) ? condition.conditions : [];
      return conditions.every((child) => evaluateVisibilityCondition(child, form));
    }}
    if (condition.kind === "any") {{
      const conditions = Array.isArray(condition.conditions) ? condition.conditions : [];
      return conditions.length === 0 || conditions.some((child) => evaluateVisibilityCondition(child, form));
    }}
    if (condition.kind === "equal") {{
      const values = Array.isArray(condition.values) ? condition.values : [];
      const value = parameterValue(form, condition.parameterId);
      if (values.length === 0 || value === undefined) {{
        return true;
      }}

      const normalizedValue = normalizeParameterValue(value);
      return values
        .map((expected) => normalizeParameterValue(String(expected)))
        .includes(normalizedValue);
    }}

    return true;
  }};

  const applyParameterVisibility = (form) => {{
    for (const wrapper of form.querySelectorAll("[data-visibility-condition]")) {{
      try {{
        wrapper.hidden = !evaluateVisibilityCondition(JSON.parse(wrapper.dataset.visibilityCondition), form);
      }} catch (_error) {{
        wrapper.hidden = false;
      }}
    }}
  }};

  const initializeParameterVisibility = (root) => {{
    for (const form of root.querySelectorAll("form")) {{
      const update = () => applyParameterVisibility(form);
      form.addEventListener("input", update);
      form.addEventListener("change", update);
      update();
    }}
  }};

  const runInlineScripts = (root) => {{
    for (const script of root.querySelectorAll("script")) {{
      if (script.src || script.type === "module") {{
        continue;
      }}
      const replacement = document.createElement("script");
      replacement.textContent = script.textContent;
      script.replaceWith(replacement);
    }}
  }};

  document.addEventListener("submit", async (event) => {{
    const form = event.target;
    const submitter = event.submitter;
    if (!(form instanceof HTMLFormElement) || !(submitter instanceof HTMLButtonElement)) {{
      return;
    }}
    if (!submitter.hasAttribute("formaction")) {{
      return;
    }}

    event.preventDefault();
    submitter.disabled = true;
    const label = submitter.textContent;
    submitter.textContent = "Working...";

    try {{
      const response = await fetch(submitter.formAction, {{
        method: "POST",
        body: new URLSearchParams(new FormData(form)),
        headers: {{
          "Accept": "text/html",
          "Content-Type": "application/x-www-form-urlencoded",
        }},
      }});
      if (!response.ok) {{
        throw new Error(`Request failed: ${{response.status}}`);
      }}

      const html = await response.text();
      const page = new DOMParser().parseFromString(html, "text/html");
      const nextMain = page.querySelector("main");
      if (!nextMain) {{
        throw new Error("Response did not include page content");
      }}

      document.querySelector("main").replaceWith(nextMain);
      if (page.title) {{
        document.title = page.title;
      }}
      window.history.replaceState(null, "", form.action || window.location.pathname);
      initializeParameterVisibility(nextMain);
      runInlineScripts(nextMain);
      configurePreviewViewers(nextMain);
    }} catch (error) {{
      submitter.disabled = false;
      submitter.textContent = label;
      alert(error.message);
    }}
  }});

  initializeParameterVisibility(document);
  configurePreviewViewers(document);
}})();
  </script>
</body>
</html>"#,
        slug = escape_html(&model.slug),
        name = escape_html(&model.name),
        description = escape_html(&model.description),
        parameter_controls = parameter_controls,
        preview = preview,
        downloads = downloads,
        download_buttons = render_download_buttons(model),
    ))
}

async fn validate_model_config(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    Form(values): Form<HashMap<String, String>>,
) -> Result<Html<String>, AppError> {
    let model = published_model(&state.catalog, &slug).ok_or(AppError::NotFound)?;
    let Some(parameters) = load_or_refresh_parameters(&state, model).await? else {
        return Ok(Html(
            "Parameter metadata is still refreshing. Try again shortly.\n".to_owned(),
        ));
    };

    match validate_values(&parameters, &values, model.parameter_policy.allow_unknown) {
        Ok(validated) => Ok(Html(format!(
            "Parameters are valid. Normalized values: <pre>{}</pre>\n",
            escape_html(
                &serde_json::to_string_pretty(&validated.values).map_err(anyhow::Error::from)?
            )
        ))),
        Err(errors) => Ok(Html(format!(
            "Parameter errors:<ul>{}</ul>\n",
            errors
                .iter()
                .map(|error| format!("<li>{}</li>", escape_html(error)))
                .collect::<String>()
        ))),
    }
}

async fn generate_preview(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    Form(values): Form<HashMap<String, String>>,
) -> Result<Html<String>, AppError> {
    let model = published_model(&state.catalog, &slug).ok_or(AppError::NotFound)?;
    let Some(parameters) = load_or_refresh_parameters(&state, model).await? else {
        return Ok(Html(
            "Parameter metadata is still refreshing. Try again shortly.\n".to_owned(),
        ));
    };
    let parameter_controls = render_parameter_controls_with_values(&parameters, &values);
    let validated =
        match validate_values(&parameters, &values, model.parameter_policy.allow_unknown) {
            Ok(validated) => validated,
            Err(errors) => {
                let preview = format!(
                    "Parameter errors:<ul>{}</ul>\n",
                    errors
                        .iter()
                        .map(|error| format!("<li>{}</li>", escape_html(error)))
                        .collect::<String>()
                );
                let downloads = render_download_prompt(model);
                return Ok(render_model_html(
                    model,
                    &parameter_controls,
                    &preview,
                    &downloads,
                ));
            }
        };
    let source_hash = resolve_source_hash(&state, model).await?;
    let config_hash = persist_configuration_selection(&state, &source_hash, &validated).await?;
    let artifact_key = preview_artifact_key(&source_hash, model, &config_hash);

    if let Some(record) = state.db.artifact(&artifact_key).await? {
        let preview = format!(
            "{}{}",
            render_clean_model_url_script(model)?,
            render_preview_result(&state, &record.object_key)
        );
        let downloads = render_downloads_for_values(&state, model, &validated).await?;
        return Ok(render_model_html(
            model,
            &parameter_controls,
            &preview,
            &downloads,
        ));
    }

    enqueue_preview(&state, model, &validated).await?;
    let status_url = preview_status_path(model, &config_hash);
    let preview = format!(
        "{}{}",
        render_clean_model_url_script(model)?,
        render_status_polling(
            "preview",
            &config_hash,
            &status_url,
            "Preview generation is queued.",
        )?
    );
    Ok(render_model_html(
        model,
        &parameter_controls,
        &preview,
        &render_download_prompt(model),
    ))
}

async fn generate_download(
    State(state): State<AppState>,
    Path((slug, format_slug)): Path<(String, String)>,
    Form(values): Form<HashMap<String, String>>,
) -> Result<Html<String>, AppError> {
    let model = published_model(&state.catalog, &slug).ok_or(AppError::NotFound)?;
    let format = catalog::DownloadFormat::from_slug(&format_slug).ok_or(AppError::NotFound)?;
    if !model.exports.downloads.contains(&format) {
        return Err(AppError::NotFound);
    }

    let Some(parameters) = load_or_refresh_parameters(&state, model).await? else {
        return Ok(Html(
            "Parameter metadata is still refreshing. Try again shortly.\n".to_owned(),
        ));
    };
    let parameter_controls = render_parameter_controls_with_values(&parameters, &values);
    let validated =
        match validate_values(&parameters, &values, model.parameter_policy.allow_unknown) {
            Ok(validated) => validated,
            Err(errors) => {
                let downloads = format!(
                    "Parameter errors:<ul>{}</ul>\n",
                    errors
                        .iter()
                        .map(|error| format!("<li>{}</li>", escape_html(error)))
                        .collect::<String>()
                );
                let preview = render_cached_preview(&state, model, &parameters).await?;
                return Ok(render_model_html(
                    model,
                    &parameter_controls,
                    &preview,
                    &downloads,
                ));
            }
        };
    let source_hash = resolve_source_hash(&state, model).await?;
    let config_hash = persist_configuration_selection(&state, &source_hash, &validated).await?;
    let artifact_key = download_artifact_key(&source_hash, model, &config_hash, format);

    if let Some(record) = state.db.artifact(&artifact_key).await? {
        let preview = format!(
            "{}{}",
            render_clean_model_url_script(model)?,
            render_preview_for_values(&state, model, &validated).await?
        );
        let downloads = render_download_result(&state, format, &record.object_key);
        return Ok(render_model_html(
            model,
            &parameter_controls,
            &preview,
            &downloads,
        ));
    }

    enqueue_download(&state, model, &validated, format).await?;
    let preview = format!(
        "{}{}",
        render_clean_model_url_script(model)?,
        render_preview_for_values(&state, model, &validated).await?
    );
    let status_url = download_status_path(model, format, &config_hash);
    Ok(render_model_html(
        model,
        &parameter_controls,
        &preview,
        &render_status_polling(
            format.slug(),
            &config_hash,
            &status_url,
            &format!("{} export generation is queued.", format.label()),
        )?,
    ))
}

async fn preview_status(
    State(state): State<AppState>,
    Path((slug, config_hash)): Path<(String, String)>,
) -> Result<Json<ArtifactStatusResponse>, AppError> {
    let model = published_model(&state.catalog, &slug).ok_or(AppError::NotFound)?;
    let source_hash = resolve_source_hash(&state, model).await?;
    let artifact_key = preview_artifact_key(&source_hash, model, &config_hash);
    let work_key = preview_work_key(&source_hash, model, &config_hash);

    Ok(Json(
        artifact_status(&state, &artifact_key, &work_key).await?,
    ))
}

async fn download_status(
    State(state): State<AppState>,
    Path((slug, format_slug, config_hash)): Path<(String, String, String)>,
) -> Result<Json<ArtifactStatusResponse>, AppError> {
    let model = published_model(&state.catalog, &slug).ok_or(AppError::NotFound)?;
    let format = catalog::DownloadFormat::from_slug(&format_slug).ok_or(AppError::NotFound)?;
    if !model.exports.downloads.contains(&format) {
        return Err(AppError::NotFound);
    }
    let source_hash = resolve_source_hash(&state, model).await?;
    let artifact_key = download_artifact_key(&source_hash, model, &config_hash, format);
    let work_key = download_work_key(&source_hash, model, &config_hash, format);

    Ok(Json(
        artifact_status(&state, &artifact_key, &work_key).await?,
    ))
}

async fn artifact_status(
    state: &AppState,
    artifact_key: &str,
    work_key: &str,
) -> Result<ArtifactStatusResponse, AppError> {
    if let Some(record) = state.db.artifact(artifact_key).await? {
        return Ok(ArtifactStatusResponse {
            artifact_key: artifact_key.to_owned(),
            status: "ready".to_owned(),
            message: "Artifact is ready.".to_owned(),
            public_url: state.storage.public_url(&record.object_key),
            object_key: Some(record.object_key),
            content_type: Some(record.content_type),
            size_bytes: record.byte_len,
            sha256: record.sha256,
            job_id: record.producing_job_key,
            source_hash: record.source_hash,
            config_hash: Some(record.config_hash),
            options_hash: record.options_hash,
            schema_version: record.parameter_schema_version,
            attempt: None,
            max_attempts: None,
            next_retry_at: None,
            error_summary: None,
            updated_at: Some(record.created_at),
        });
    }

    if let Some(job) = state.db.job(work_key).await? {
        let message = match job.status.as_str() {
            "queued" => "Generation is queued.",
            "running" => "Generation is running.",
            "failed" => "Generation failed.",
            "ready" => "Generation completed; artifact is not visible yet.",
            "superseded" => "Generation was superseded and needs to be queued again.",
            _ => "Generation status is unknown.",
        };
        return Ok(ArtifactStatusResponse {
            artifact_key: artifact_key.to_owned(),
            status: job.status,
            message: message.to_owned(),
            public_url: None,
            object_key: None,
            content_type: None,
            size_bytes: None,
            sha256: None,
            job_id: Some(work_key.to_owned()),
            source_hash: None,
            config_hash: None,
            options_hash: None,
            schema_version: None,
            attempt: Some(job.attempt),
            max_attempts: Some(job.max_attempts),
            next_retry_at: job.next_retry_at,
            error_summary: job.error_summary,
            updated_at: Some(job.updated_at),
        });
    }

    Ok(ArtifactStatusResponse {
        artifact_key: artifact_key.to_owned(),
        status: "missing".to_owned(),
        message: "No cached artifact or queued generation was found.".to_owned(),
        public_url: None,
        object_key: None,
        content_type: None,
        size_bytes: None,
        sha256: None,
        job_id: None,
        source_hash: None,
        config_hash: None,
        options_hash: None,
        schema_version: None,
        attempt: None,
        max_attempts: None,
        next_retry_at: None,
        error_summary: None,
        updated_at: None,
    })
}

#[derive(Debug, thiserror::Error)]
enum AppError {
    #[error("not found")]
    NotFound,
    #[error(transparent)]
    Database(#[from] sqlx::Error),
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        match self {
            Self::NotFound => (StatusCode::NOT_FOUND, "not found\n").into_response(),
            Self::Database(error) => {
                tracing::error!(%error, "database error");
                (StatusCode::INTERNAL_SERVER_ERROR, "internal server error\n").into_response()
            }
            Self::Other(error) => {
                tracing::error!(%error, "application error");
                (StatusCode::INTERNAL_SERVER_ERROR, "internal server error\n").into_response()
            }
        }
    }
}

async fn load_or_refresh_parameters(
    state: &AppState,
    model: &catalog::Model,
) -> Result<Option<ParameterSchema>, AppError> {
    let source_hash = resolve_source_hash(state, model).await?;
    if let Some(record) = state.db.parameter_metadata(&source_hash).await? {
        let schema = state
            .storage
            .get_json::<ParameterSchema>(&record.normalized_object_key)
            .await?;
        if !parameter_schema_is_current(&schema) {
            tracing::info!(
                model = %model.slug,
                cached_schema_version = schema.schema_version,
                current_schema_version = SCHEMA_VERSION,
                "refreshing stale parameter schema"
            );
            return rebuild_normalized_parameters_from_raw(state, model, &record.raw_object_key)
                .await;
        }

        let mut schema = schema;
        validate_parameter_overrides(model, &schema)?;
        apply_overrides(&mut schema, &model.parameter_overrides);
        return Ok(Some(schema));
    }

    enqueue_parameter_refresh(state, model).await?;
    Ok(None)
}

fn parameter_schema_is_current(schema: &ParameterSchema) -> bool {
    schema.schema_version == SCHEMA_VERSION
}

fn validate_parameter_overrides(
    model: &catalog::Model,
    schema: &ParameterSchema,
) -> anyhow::Result<()> {
    let known = schema
        .parameters
        .iter()
        .map(|parameter| parameter.id.as_str())
        .collect::<HashSet<_>>();

    for parameter_id in model.parameter_overrides.keys() {
        anyhow::ensure!(
            known.contains(parameter_id.as_str()),
            "parameter override for {} references unknown parameter: {}",
            model.slug,
            parameter_id
        );
    }

    Ok(())
}

async fn rebuild_normalized_parameters_from_raw(
    state: &AppState,
    model: &catalog::Model,
    raw_key: &str,
) -> Result<Option<ParameterSchema>, AppError> {
    let raw = match state.storage.get_json::<Value>(raw_key).await {
        Ok(raw) => raw,
        Err(error) => {
            tracing::warn!(model = %model.slug, raw_key, %error, "cached raw parameter metadata unavailable");
            enqueue_parameter_refresh(state, model).await?;
            return Ok(None);
        }
    };

    let mut schema = normalize_configuration(&model.onshape, &raw);
    validate_parameter_overrides(model, &schema)?;
    apply_overrides(&mut schema, &model.parameter_overrides);
    let source_hash = resolve_source_hash(state, model).await?;
    let schema_hash = parameter_schema_hash(&schema)?;
    let normalized_key = parameter_normalized_key(&source_hash, &schema_hash);
    state.storage.put_json(&normalized_key, &schema).await?;
    state
        .db
        .upsert_parameter_metadata(
            &source_hash,
            raw_key,
            &normalized_key,
            &schema_hash,
            SCHEMA_VERSION.into(),
        )
        .await?;
    Ok(Some(schema))
}

async fn background_runtime(
    state: AppState,
    rebuild_interval: Option<Duration>,
    worker_concurrency: usize,
) {
    if let Some(rebuild_interval) = rebuild_interval {
        tokio::select! {
            () = worker_loop(state.clone(), worker_concurrency) => {},
            () = scheduled_rebuild_loop(state, rebuild_interval) => {},
        }
    } else {
        worker_loop(state, worker_concurrency).await;
    }
}

async fn scheduled_rebuild_loop(state: AppState, rebuild_interval: Duration) {
    tracing::info!(
        interval_seconds = rebuild_interval.as_secs(),
        "scheduled rebuilds enabled"
    );
    let mut interval = tokio::time::interval(rebuild_interval);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        interval.tick().await;
        if let Err(error) = enqueue_scheduled_rebuild(&state).await {
            tracing::error!(%error, "scheduled rebuild enqueue failed");
        }
    }
}

async fn enqueue_scheduled_rebuild(state: &AppState) -> anyhow::Result<()> {
    let mut enqueued = 0usize;
    for model in state.catalog.models() {
        if model.parameter_policy.auto_refresh
            && force_enqueue_parameter_refresh(state, model).await?
        {
            enqueued += 1;
        }

        let Some(validated) = cached_default_parameter_values(state, model).await? else {
            tracing::debug!(model = %model.slug, "default artifact rebuild skipped until parameter metadata is cached");
            continue;
        };

        let source_hash = resolve_source_hash(state, model).await?;
        let config_hash = persist_configuration_selection(state, &source_hash, &validated).await?;
        let preview_artifact_key = preview_artifact_key(&source_hash, model, &config_hash);
        if state.db.artifact(&preview_artifact_key).await?.is_none()
            && enqueue_preview(state, model, &validated).await?
        {
            enqueued += 1;
        }
        for format in &model.exports.downloads {
            let artifact_key = download_artifact_key(&source_hash, model, &config_hash, *format);
            if state.db.artifact(&artifact_key).await?.is_none()
                && enqueue_download(state, model, &validated, *format).await?
            {
                enqueued += 1;
            }
        }
    }

    tracing::info!(enqueued, "scheduled rebuild enqueue complete");
    Ok(())
}

async fn cached_default_parameter_values(
    state: &AppState,
    model: &catalog::Model,
) -> anyhow::Result<Option<ValidatedConfiguration>> {
    let source_hash = resolve_source_hash(state, model).await?;
    let Some(record) = state.db.parameter_metadata(&source_hash).await? else {
        return Ok(None);
    };
    let schema = state
        .storage
        .get_json::<ParameterSchema>(&record.normalized_object_key)
        .await?;
    validate_parameter_overrides(model, &schema)?;

    match validate_values(
        &schema,
        &HashMap::new(),
        model.parameter_policy.allow_unknown,
    ) {
        Ok(validated) => Ok(Some(validated)),
        Err(errors) => {
            tracing::warn!(model = %model.slug, errors = ?errors, "scheduled default parameter validation failed");
            Ok(None)
        }
    }
}

async fn worker_loop(state: AppState, worker_concurrency: usize) {
    if worker_concurrency == 1 {
        single_worker_loop(state, 0).await;
        return;
    }

    tracing::info!(worker_concurrency, "starting worker loops");
    for worker_index in 0..worker_concurrency {
        tokio::spawn(single_worker_loop(state.clone(), worker_index));
    }
    std::future::pending::<()>().await;
}

async fn single_worker_loop(state: AppState, worker_index: usize) {
    let mut interval = tokio::time::interval(Duration::from_secs(5));
    loop {
        interval.tick().await;
        loop {
            match process_next_job(&state).await {
                Ok(true) => {}
                Ok(false) => break,
                Err(error) => {
                    tracing::error!(%error, worker_index, "worker failed to process job");
                    break;
                }
            }
        }
    }
}

async fn process_next_job(state: &AppState) -> anyhow::Result<bool> {
    let Some(job) = state.db.claim_next_job(15 * 60).await? else {
        return Ok(false);
    };

    let result = execute_job(state, &job).await;
    match result {
        Ok(()) => {
            if !state
                .db
                .finish_job(&job.work_key, job.attempt, "ready", None)
                .await?
            {
                tracing::warn!(work_key = %job.work_key, attempt = job.attempt, "job lease was already reclaimed before completion");
            }
        }
        Err(error) => {
            let summary = error.to_string();
            let will_retry = job.attempt < job.max_attempts;
            let retry_delay_seconds = if will_retry {
                retry_backoff_seconds(job.attempt)
            } else {
                0
            };
            tracing::error!(
                error = %error,
                work_key = %job.work_key,
                job_kind = %job.job_kind,
                attempt = job.attempt,
                max_attempts = job.max_attempts,
                retry_delay_seconds,
                "job failed"
            );
            if !state
                .db
                .record_job_failure(&job.work_key, job.attempt, &summary, retry_delay_seconds)
                .await?
            {
                tracing::warn!(work_key = %job.work_key, attempt = job.attempt, "job lease was already reclaimed before failure was recorded");
            }
            if will_retry {
                return Ok(true);
            }
            return Err(error);
        }
    }

    Ok(true)
}

fn retry_backoff_seconds(attempt: i64) -> i64 {
    let exponent = attempt.saturating_sub(1).min(10) as u32;
    let multiplier = 1_i64.checked_shl(exponent).unwrap_or(i64::MAX);
    let max_delay = RETRY_BACKOFF_BASE_SECONDS
        .saturating_mul(multiplier)
        .min(RETRY_BACKOFF_CAP_SECONDS);
    fastrand::i64(0..=max_delay)
}

async fn execute_job(state: &AppState, job: &db::JobLease) -> anyhow::Result<()> {
    let payload: JobPayload = serde_json::from_str(&job.payload_json)?;
    match payload {
        JobPayload::ParameterRefresh { model_slug } => {
            let model = state
                .catalog
                .find(&model_slug)
                .ok_or_else(|| anyhow::anyhow!("unknown model slug: {model_slug}"))?;
            refresh_parameters(state, model).await?;
        }
        JobPayload::PreviewGlb {
            model_slug,
            config_hash,
            config_hash_version,
            values,
        } => {
            anyhow::ensure!(
                job.job_kind == "preview_export",
                "unexpected preview job kind"
            );
            let model = state
                .catalog
                .find(&model_slug)
                .ok_or_else(|| anyhow::anyhow!("unknown model slug: {model_slug}"))?;
            let parameters = load_or_refresh_parameters(state, model)
                .await?
                .ok_or_else(|| anyhow::anyhow!("parameter metadata refresh is still queued"))?;
            let validated =
                validate_values(&parameters, &values, model.parameter_policy.allow_unknown)
                    .map_err(|errors| {
                        anyhow::anyhow!(
                            "{} preview parameters are invalid: {}",
                            model.slug,
                            errors.join(", ")
                        )
                    })?;
            let source_hash = resolve_source_hash(state, model).await?;
            let recomputed_config_hash = configuration_hash(&source_hash, &validated)?;
            if config_hash_version == Some(CONFIG_HASH_JOB_VERSION) && !config_hash.is_empty() {
                anyhow::ensure!(
                    recomputed_config_hash == config_hash,
                    "queued preview config hash no longer matches current parameter schema"
                );
            }
            let config_hash = if config_hash.is_empty() {
                recomputed_config_hash
            } else {
                config_hash
            };
            persist_configuration_selection(state, &source_hash, &validated).await?;
            let artifact_key = preview_artifact_key(&source_hash, model, &config_hash);
            if state.db.artifact(&artifact_key).await?.is_none() {
                refresh_preview(
                    state,
                    model,
                    &source_hash,
                    &validated.values,
                    &config_hash,
                    &artifact_key,
                    Some(&job.work_key),
                )
                .await?;
            }
        }
        JobPayload::DownloadExport {
            model_slug,
            config_hash,
            config_hash_version,
            values,
            format,
        } => {
            anyhow::ensure!(
                job.job_kind == "download_export",
                "unexpected download job kind"
            );
            let model = state
                .catalog
                .find(&model_slug)
                .ok_or_else(|| anyhow::anyhow!("unknown model slug: {model_slug}"))?;
            anyhow::ensure!(
                model.exports.downloads.contains(&format),
                "{} does not expose {} downloads",
                model.slug,
                format.label()
            );
            let parameters = load_or_refresh_parameters(state, model)
                .await?
                .ok_or_else(|| anyhow::anyhow!("parameter metadata refresh is still queued"))?;
            let validated =
                validate_values(&parameters, &values, model.parameter_policy.allow_unknown)
                    .map_err(|errors| {
                        anyhow::anyhow!(
                            "{} download parameters are invalid: {}",
                            model.slug,
                            errors.join(", ")
                        )
                    })?;
            let source_hash = resolve_source_hash(state, model).await?;
            let recomputed_config_hash = configuration_hash(&source_hash, &validated)?;
            if config_hash_version == Some(CONFIG_HASH_JOB_VERSION) && !config_hash.is_empty() {
                anyhow::ensure!(
                    recomputed_config_hash == config_hash,
                    "queued download config hash no longer matches current parameter schema"
                );
            }
            let config_hash = if config_hash.is_empty() {
                recomputed_config_hash
            } else {
                config_hash
            };
            persist_configuration_selection(state, &source_hash, &validated).await?;
            let artifact_key = download_artifact_key(&source_hash, model, &config_hash, format);
            if state.db.artifact(&artifact_key).await?.is_none() {
                refresh_download(
                    state,
                    model,
                    &source_hash,
                    &validated.values,
                    &config_hash,
                    &artifact_key,
                    format,
                    Some(&job.work_key),
                )
                .await?;
            }
        }
    }
    Ok(())
}

async fn refresh_parameters(
    state: &AppState,
    model: &catalog::Model,
) -> anyhow::Result<ParameterSchema> {
    let source_hash = resolve_source_hash(state, model).await?;
    let raw = state.onshape.fetch_configuration(&model.onshape).await?;
    let mut schema = normalize_configuration(&model.onshape, &raw);
    validate_parameter_overrides(model, &schema)?;
    apply_overrides(&mut schema, &model.parameter_overrides);
    let schema_hash = parameter_schema_hash(&schema)?;
    let raw_key = parameter_raw_key(&source_hash);
    let normalized_key = parameter_normalized_key(&source_hash, &schema_hash);

    state.storage.put_json(&raw_key, &raw).await?;
    state.storage.put_json(&normalized_key, &schema).await?;
    state
        .db
        .upsert_parameter_metadata(
            &source_hash,
            &raw_key,
            &normalized_key,
            &schema_hash,
            SCHEMA_VERSION.into(),
        )
        .await?;

    Ok(schema)
}

fn parameter_raw_key(source_hash: &str) -> String {
    format!("onshape/source/v1/{source_hash}/configuration.raw.json")
}

fn parameter_normalized_key(source_hash: &str, schema_hash: &str) -> String {
    format!("onshape/source/v1/{source_hash}/parameters.normalized/{schema_hash}.json")
}

async fn render_cached_preview(
    state: &AppState,
    model: &catalog::Model,
    parameters: &ParameterSchema,
) -> Result<String, AppError> {
    let submitted = HashMap::new();
    let Ok(validated) =
        validate_values(parameters, &submitted, model.parameter_policy.allow_unknown)
    else {
        return Ok("<p>Choose parameters and generate a preview.</p>".to_owned());
    };
    let source_hash = resolve_source_hash(state, model).await?;
    let config_hash = persist_configuration_selection(state, &source_hash, &validated).await?;
    render_preview_for_hash(state, model, &config_hash, "default parameters").await
}

async fn render_preview_for_values(
    state: &AppState,
    model: &catalog::Model,
    validated: &ValidatedConfiguration,
) -> Result<String, AppError> {
    let source_hash = resolve_source_hash(state, model).await?;
    let config_hash = persist_configuration_selection(state, &source_hash, validated).await?;
    render_preview_for_hash(state, model, &config_hash, "these parameters").await
}

async fn render_preview_for_hash(
    state: &AppState,
    model: &catalog::Model,
    config_hash: &str,
    label: &str,
) -> Result<String, AppError> {
    let source_hash = resolve_source_hash(state, model).await?;
    let artifact_key = preview_artifact_key(&source_hash, model, config_hash);

    match state.db.artifact(&artifact_key).await? {
        Some(record) => Ok(render_preview_viewer(state, &record.object_key)),
        None => Ok(format!("<p>No cached preview for {label} yet.</p>")),
    }
}

async fn render_cached_downloads(
    state: &AppState,
    model: &catalog::Model,
    parameters: &ParameterSchema,
) -> Result<String, AppError> {
    let submitted = HashMap::new();
    let Ok(validated) =
        validate_values(parameters, &submitted, model.parameter_policy.allow_unknown)
    else {
        return Ok(render_download_prompt(model));
    };

    render_downloads_for_values(state, model, &validated).await
}

async fn render_downloads_for_values(
    state: &AppState,
    model: &catalog::Model,
    validated: &ValidatedConfiguration,
) -> Result<String, AppError> {
    let source_hash = resolve_source_hash(state, model).await?;
    let config_hash = persist_configuration_selection(state, &source_hash, validated).await?;
    let mut items = String::new();
    for format in &model.exports.downloads {
        let artifact_key = download_artifact_key(&source_hash, model, &config_hash, *format);
        if let Some(record) = state.db.artifact(&artifact_key).await? {
            items.push_str(&format!(
                "<li>{}</li>",
                render_download_link(state, *format, &record.object_key)
            ));
        }
    }

    if items.is_empty() {
        Ok(render_download_prompt(model))
    } else {
        Ok(format!("<ul>{items}</ul>"))
    }
}

async fn refresh_preview(
    state: &AppState,
    model: &catalog::Model,
    source_hash: &str,
    values: &HashMap<String, String>,
    config_hash: &str,
    artifact_key: &str,
    producing_job_key: Option<&str>,
) -> anyhow::Result<String> {
    let configuration =
        resolve_configuration_encoding(state, model, source_hash, config_hash, values).await?;
    let bytes = state
        .onshape
        .export_glb(
            &model.onshape,
            &configuration.encoded_id,
            &model.exports.preview_options,
        )
        .await?;
    let preview_artifact =
        preview_artifact_from_onshape_bytes(source_hash, model, config_hash, bytes)?;
    let sha256 = cache_key::hex_sha256(&preview_artifact.bytes);
    let options_hash = preview_options_hash(model);
    let config_values_json = config_values_json(values)?;
    state
        .storage
        .put_bytes(
            &preview_artifact.object_key,
            preview_artifact.bytes.clone(),
            preview_artifact.content_type,
        )
        .await?;
    for sidecar in preview_artifact.sidecars {
        state
            .storage
            .put_bytes(&sidecar.object_key, sidecar.bytes, sidecar.content_type)
            .await?;
    }
    state
        .db
        .upsert_artifact(ArtifactUpsert {
            artifact_key,
            model_slug: &model.slug,
            config_hash,
            output_kind: "preview_glb",
            object_key: &preview_artifact.object_key,
            content_type: preview_artifact.content_type,
            byte_len: preview_artifact.bytes.len() as i64,
            sha256: &sha256,
            producing_job_key,
            source_hash,
            options_hash: &options_hash,
            parameter_schema_version: SCHEMA_VERSION.into(),
            config_values_json: &config_values_json,
        })
        .await?;
    rewrite_manifest(state, model, config_hash, Some(values)).await?;
    Ok(preview_artifact.object_key)
}

fn preview_artifact_from_onshape_bytes(
    source_hash: &str,
    model: &catalog::Model,
    config_hash: &str,
    bytes: Vec<u8>,
) -> anyhow::Result<PreviewArtifact> {
    if bytes.starts_with(b"glTF") {
        validate_glb(&bytes).context("validating direct GLB preview export")?;
        return Ok(PreviewArtifact {
            object_key: preview_glb_object_key(source_hash, model, config_hash),
            content_type: "model/gltf-binary",
            bytes,
            sidecars: Vec::new(),
        });
    }

    if bytes.starts_with(b"PK\x03\x04") {
        return preview_artifact_from_zip(source_hash, model, config_hash, bytes);
    }

    validate_gltf_json(&bytes).context("validating direct glTF preview export")?;
    Ok(PreviewArtifact {
        object_key: preview_gltf_object_key(source_hash, model, config_hash),
        content_type: "model/gltf+json",
        bytes,
        sidecars: Vec::new(),
    })
}

fn validate_gltf_json(bytes: &[u8]) -> anyhow::Result<()> {
    let value = serde_json::from_slice::<Value>(bytes)?;
    anyhow::ensure!(
        value
            .get("asset")
            .and_then(|asset| asset.get("version"))
            .and_then(Value::as_str)
            .is_some(),
        "glTF JSON did not include asset.version"
    );
    Ok(())
}

fn preview_artifact_from_zip(
    source_hash: &str,
    model: &catalog::Model,
    config_hash: &str,
    bytes: Vec<u8>,
) -> anyhow::Result<PreviewArtifact> {
    let source_zip = PreviewAsset {
        object_key: preview_source_zip_object_key(source_hash, model, config_hash),
        content_type: "application/zip",
        bytes: bytes.clone(),
    };
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))?;
    let glb_entries = zip_entry_indices_with_extension(&mut archive, "glb")?;
    match glb_entries.as_slice() {
        [index] => {
            let bytes = read_zip_entry(&mut archive, *index)?;
            validate_glb(&bytes).context("validating zipped GLB preview export")?;
            Ok(PreviewArtifact {
                object_key: preview_glb_object_key(source_hash, model, config_hash),
                content_type: "model/gltf-binary",
                bytes,
                sidecars: vec![source_zip],
            })
        }
        [] => preview_artifact_from_gltf_zip(source_hash, model, config_hash, archive, source_zip),
        _ => anyhow::bail!(
            "Onshape preview ZIP contained multiple GLB files; expected exactly one GLB preview artifact"
        ),
    }
}

fn preview_artifact_from_gltf_zip(
    source_hash: &str,
    model: &catalog::Model,
    config_hash: &str,
    mut archive: zip::ZipArchive<Cursor<Vec<u8>>>,
    source_zip: PreviewAsset,
) -> anyhow::Result<PreviewArtifact> {
    let gltf_entries = zip_entry_indices_with_extension(&mut archive, "gltf")?;
    let primary_index = match gltf_entries.as_slice() {
        [] => anyhow::bail!("Onshape preview ZIP contained neither a GLB nor a glTF preview file"),
        [index] => *index,
        indices if ALLOW_PARTIAL_MULTI_GLTF_PREVIEW_FALLBACK => {
            largest_zip_entry(&mut archive, indices)?.expect("indices are non-empty")
        }
        _ => anyhow::bail!(
            "Onshape preview ZIP contained multiple glTF files; grouped preview export did not produce a single viewer asset"
        ),
    };
    let primary_name = safe_zip_asset_name(archive.by_index(primary_index)?.name())?;
    let primary_bytes = read_zip_entry(&mut archive, primary_index)?;
    validate_gltf_json(&primary_bytes).context("validating zipped glTF preview JSON")?;

    let mut sidecars = vec![source_zip];
    for index in 0..archive.len() {
        if index == primary_index {
            continue;
        }
        let mut file = archive.by_index(index)?;
        if file.is_dir() {
            continue;
        }
        let asset_name = safe_zip_asset_name(file.name())?;
        let mut bytes = Vec::with_capacity(file.size() as usize);
        file.read_to_end(&mut bytes)?;
        sidecars.push(PreviewAsset {
            object_key: preview_asset_object_key(source_hash, model, config_hash, &asset_name),
            content_type: preview_asset_content_type(&asset_name),
            bytes,
        });
    }

    Ok(PreviewArtifact {
        object_key: preview_asset_object_key(source_hash, model, config_hash, &primary_name),
        content_type: "model/gltf+json",
        bytes: primary_bytes,
        sidecars,
    })
}

fn largest_zip_entry(
    archive: &mut zip::ZipArchive<Cursor<Vec<u8>>>,
    indices: &[usize],
) -> anyhow::Result<Option<usize>> {
    let mut largest = None;
    for index in indices {
        let size = archive.by_index(*index)?.size();
        if largest
            .map(|(_, largest_size)| size > largest_size)
            .unwrap_or(true)
        {
            largest = Some((*index, size));
        }
    }
    Ok(largest.map(|(index, _)| index))
}

fn safe_zip_asset_name(name: &str) -> anyhow::Result<String> {
    anyhow::ensure!(
        !name.contains('\\'),
        "ZIP asset path contains a backslash: {name}"
    );
    anyhow::ensure!(!name.starts_with('/'), "ZIP asset path is absolute: {name}");
    let parts = name.split('/').collect::<Vec<_>>();
    anyhow::ensure!(!parts.is_empty(), "ZIP asset path is empty");
    for part in &parts {
        anyhow::ensure!(
            !part.is_empty() && *part != "." && *part != "..",
            "ZIP asset path is not safe: {name}"
        );
        anyhow::ensure!(
            !part.chars().any(char::is_control),
            "ZIP asset path contains control characters: {name}"
        );
    }
    Ok(parts.join("/"))
}

fn preview_asset_content_type(name: &str) -> &'static str {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".gltf") {
        "model/gltf+json"
    } else if lower.ends_with(".bin") {
        "application/octet-stream"
    } else if lower.ends_with(".png") {
        "image/png"
    } else if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        "image/jpeg"
    } else if lower.ends_with(".webp") {
        "image/webp"
    } else {
        "application/octet-stream"
    }
}

fn validate_glb(bytes: &[u8]) -> anyhow::Result<()> {
    anyhow::ensure!(bytes.len() >= 12, "GLB data is shorter than its header");
    anyhow::ensure!(
        &bytes[0..4] == b"glTF",
        "GLB data has an invalid magic header"
    );
    let version = u32::from_le_bytes(bytes[4..8].try_into().expect("slice length checked"));
    anyhow::ensure!(version == 2, "unsupported GLB version: {version}");
    let declared_len = u32::from_le_bytes(bytes[8..12].try_into().expect("slice length checked"));
    anyhow::ensure!(
        declared_len as usize == bytes.len(),
        "GLB declared length {declared_len} does not match {} bytes",
        bytes.len()
    );
    Ok(())
}

fn zip_entry_indices_with_extension(
    archive: &mut zip::ZipArchive<Cursor<Vec<u8>>>,
    extension: &str,
) -> anyhow::Result<Vec<usize>> {
    let suffix = format!(".{extension}");
    let mut indices = Vec::new();
    for index in 0..archive.len() {
        let file = archive.by_index(index)?;
        if file.is_dir() {
            continue;
        }
        let name = file.name().to_ascii_lowercase();
        if name.ends_with(&suffix) {
            indices.push(index);
        }
    }
    Ok(indices)
}

fn read_zip_entry(
    archive: &mut zip::ZipArchive<Cursor<Vec<u8>>>,
    index: usize,
) -> anyhow::Result<Vec<u8>> {
    let mut file = archive.by_index(index)?;
    let mut bytes = Vec::with_capacity(file.size() as usize);
    file.read_to_end(&mut bytes)?;
    Ok(bytes)
}

async fn refresh_download(
    state: &AppState,
    model: &catalog::Model,
    source_hash: &str,
    values: &HashMap<String, String>,
    config_hash: &str,
    artifact_key: &str,
    format: catalog::DownloadFormat,
    producing_job_key: Option<&str>,
) -> anyhow::Result<String> {
    let configuration =
        resolve_configuration_encoding(state, model, source_hash, config_hash, values).await?;
    let bytes = state
        .onshape
        .export_download(
            &model.onshape,
            &configuration.encoded_id,
            format,
            &model.exports.download_options,
        )
        .await?;
    let object_key = download_object_key(source_hash, model, config_hash, format);
    let filename = download_filename(model, format);
    let content_disposition = format!("attachment; filename=\"{filename}\"");
    let sha256 = cache_key::hex_sha256(&bytes);
    let options_hash = download_options_hash(model, format);
    let config_values_json = config_values_json(values)?;
    state
        .storage
        .put_bytes_with_headers(
            &object_key,
            bytes.clone(),
            format.content_type(),
            Some(&content_disposition),
            Some("public, max-age=31536000, immutable"),
        )
        .await?;
    state
        .db
        .upsert_artifact(ArtifactUpsert {
            artifact_key,
            model_slug: &model.slug,
            config_hash,
            output_kind: format.slug(),
            object_key: &object_key,
            content_type: format.content_type(),
            byte_len: bytes.len() as i64,
            sha256: &sha256,
            producing_job_key,
            source_hash,
            options_hash: &options_hash,
            parameter_schema_version: SCHEMA_VERSION.into(),
            config_values_json: &config_values_json,
        })
        .await?;
    rewrite_manifest(state, model, config_hash, Some(values)).await?;
    Ok(object_key)
}

async fn rewrite_manifest(
    state: &AppState,
    model: &catalog::Model,
    config_hash: &str,
    values: Option<&HashMap<String, String>>,
) -> anyhow::Result<String> {
    let artifacts = state
        .db
        .artifacts_for_configuration(&model.slug, config_hash)
        .await?;
    let source_hash = artifacts
        .first()
        .and_then(|artifact| artifact.source_hash.clone())
        .unwrap_or(resolve_source_hash(state, model).await?);
    let manifest = build_manifest(
        &source_hash,
        model,
        config_hash,
        values,
        &artifacts,
        state.storage.public_base_url(),
    );
    let object_key = manifest_object_key(&source_hash, config_hash);
    state.storage.put_json(&object_key, &manifest).await?;
    Ok(object_key)
}

async fn supersede_artifact_and_rewrite_manifest(
    state: &AppState,
    artifact: &db::ArtifactRecord,
) -> anyhow::Result<()> {
    state.db.supersede_artifact(&artifact.artifact_key).await?;
    if let Some(model) = state.catalog.find(&artifact.model_slug) {
        let producing_job_key = artifact
            .producing_job_key
            .clone()
            .or_else(|| artifact_work_key(model, artifact));
        if let Some(work_key) = producing_job_key {
            state.db.supersede_ready_job(&work_key).await?;
        }
        rewrite_manifest(state, model, &artifact.config_hash, None).await?;
    }
    Ok(())
}

fn artifact_work_key(model: &catalog::Model, artifact: &db::ArtifactRecord) -> Option<String> {
    let source_hash = artifact.source_hash.as_deref()?;
    if artifact.output_kind == "preview_glb" {
        return Some(preview_work_key(source_hash, model, &artifact.config_hash));
    }

    catalog::DownloadFormat::from_slug(&artifact.output_kind)
        .map(|format| download_work_key(source_hash, model, &artifact.config_hash, format))
}

fn build_manifest(
    source_hash: &str,
    model: &catalog::Model,
    config_hash: &str,
    values: Option<&HashMap<String, String>>,
    artifacts: &[db::ArtifactRecord],
    public_base_url: Option<&str>,
) -> ArtifactManifest {
    let configuration_values = values
        .map(cache_model::canonical_values)
        .unwrap_or_else(|| manifest_values_from_artifacts(artifacts));
    let outputs = artifacts
        .iter()
        .map(|artifact| {
            (
                artifact.output_kind.clone(),
                ManifestOutput {
                    artifact_key: artifact.artifact_key.clone(),
                    object_key: artifact.object_key.clone(),
                    public_url: public_base_url
                        .map(|base| public_url_from_base(base, &artifact.object_key)),
                    status: artifact.status.clone(),
                    content_type: artifact.content_type.clone(),
                    size_bytes: artifact.byte_len,
                    sha256: artifact.sha256.clone(),
                    job_id: artifact.producing_job_key.clone(),
                    source_hash: artifact.source_hash.clone(),
                    options_hash: artifact.options_hash.clone(),
                    schema_version: artifact.parameter_schema_version,
                    created_at: artifact.created_at.clone(),
                    superseded_at: artifact.superseded_at.clone(),
                },
            )
        })
        .collect::<BTreeMap<_, _>>();
    let created_at = artifacts
        .iter()
        .map(|artifact| artifact.created_at.as_str())
        .min()
        .map(str::to_owned);

    ArtifactManifest {
        manifest_schema_version: MANIFEST_SCHEMA_VERSION,
        group_id: manifest_group_id(source_hash, config_hash),
        model_slug: model.slug.clone(),
        onshape: ManifestOnshapeSource {
            document_id: model.onshape.document_id.clone(),
            version_id: model.onshape.version_id.clone(),
            element_id: model.onshape.element_id.clone(),
            element_kind: model.onshape.element_kind.key().to_owned(),
            link_document_id: model.onshape.link_document_id.clone(),
        },
        configuration: ManifestConfiguration {
            hash: config_hash.to_owned(),
            values: configuration_values,
        },
        outputs,
        created_at,
        exporter_version: EXPORTER_VERSION,
    }
}

fn manifest_values_from_artifacts(artifacts: &[db::ArtifactRecord]) -> BTreeMap<String, String> {
    artifacts
        .iter()
        .filter_map(|artifact| artifact.config_values_json.as_deref())
        .find_map(|values| serde_json::from_str(values).ok())
        .unwrap_or_default()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WorkKeyPayload {
    source_hash: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    config_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    options_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    format: Option<&'static str>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DownloadArtifactIdPayload<'a> {
    source_hash: &'a str,
    config_hash: &'a str,
    options_hash: &'a str,
    format: &'a str,
}

fn manifest_group_id(source_hash: &str, config_hash: &str) -> String {
    format!("group-v1:{source_hash}:{config_hash}")
}

fn manifest_object_key(source_hash: &str, config_hash: &str) -> String {
    format!(
        "manifests/v1/{}.json",
        manifest_group_id(source_hash, config_hash)
    )
}

fn parameter_refresh_work_key(source_hash: &str) -> String {
    work_key(
        "parameter_refresh",
        &WorkKeyPayload {
            source_hash: source_hash.to_owned(),
            config_hash: None,
            options_hash: None,
            format: None,
        },
    )
}

fn preview_work_key(source_hash: &str, model: &catalog::Model, config_hash: &str) -> String {
    work_key(
        "preview_export",
        &WorkKeyPayload {
            source_hash: source_hash.to_owned(),
            config_hash: Some(config_hash.to_owned()),
            options_hash: Some(preview_options_hash(model)),
            format: Some("glb"),
        },
    )
}

fn download_work_key(
    source_hash: &str,
    model: &catalog::Model,
    config_hash: &str,
    format: catalog::DownloadFormat,
) -> String {
    work_key(
        "download_export",
        &WorkKeyPayload {
            source_hash: source_hash.to_owned(),
            config_hash: Some(config_hash.to_owned()),
            options_hash: Some(download_options_hash(model, format)),
            format: Some(format.slug()),
        },
    )
}

fn preview_artifact_key(source_hash: &str, model: &catalog::Model, config_hash: &str) -> String {
    format!(
        "artifact-v1:preview_glb:{}:{config_hash}:{}",
        source_hash,
        preview_options_hash(model),
    )
}

fn preview_glb_object_key(source_hash: &str, model: &catalog::Model, config_hash: &str) -> String {
    format!(
        "previews/v1/{}/{}/{}/preview.glb",
        source_hash,
        config_hash,
        preview_options_hash(model),
    )
}

fn preview_gltf_object_key(source_hash: &str, model: &catalog::Model, config_hash: &str) -> String {
    preview_asset_object_key(source_hash, model, config_hash, "preview.gltf")
}

fn preview_asset_object_key(
    source_hash: &str,
    model: &catalog::Model,
    config_hash: &str,
    asset_name: &str,
) -> String {
    format!(
        "previews/v1/{}/{}/{}/{}",
        source_hash,
        config_hash,
        preview_options_hash(model),
        asset_name,
    )
}

fn preview_source_zip_object_key(
    source_hash: &str,
    model: &catalog::Model,
    config_hash: &str,
) -> String {
    preview_asset_object_key(source_hash, model, config_hash, "source.zip")
}

fn download_artifact_key(
    source_hash: &str,
    model: &catalog::Model,
    config_hash: &str,
    format: catalog::DownloadFormat,
) -> String {
    format!(
        "artifact-v1:download:{}:{}:{config_hash}:{}",
        format.slug(),
        source_hash,
        download_options_hash(model, format),
    )
}

fn download_object_key(
    source_hash: &str,
    model: &catalog::Model,
    config_hash: &str,
    format: catalog::DownloadFormat,
) -> String {
    let options_hash = download_options_hash(model, format);
    let artifact_id = download_artifact_id(source_hash, config_hash, &options_hash, format);
    format!(
        "artifacts/v1/{source_hash}/{config_hash}/{}/{options_hash}/{}.{}",
        format.slug(),
        artifact_id,
        format.extension()
    )
}

fn download_filename(model: &catalog::Model, format: catalog::DownloadFormat) -> String {
    format!(
        "{}-{}.{}",
        safe_filename_stem(&model.slug),
        format.slug(),
        format.extension()
    )
}

fn public_url_from_base(base: &str, object_key: &str) -> String {
    format!(
        "{}/{}",
        base.trim_end_matches('/'),
        object_key
            .split('/')
            .map(url_path_segment)
            .collect::<Vec<_>>()
            .join("/")
    )
}

fn url_path_segment(value: &str) -> String {
    value
        .bytes()
        .flat_map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                vec![byte as char]
            }
            byte => format!("%{byte:02X}").chars().collect(),
        })
        .collect()
}

fn preview_status_path(model: &catalog::Model, config_hash: &str) -> String {
    format!("/models/{}/preview/{config_hash}/status", model.slug)
}

fn download_status_path(
    model: &catalog::Model,
    format: catalog::DownloadFormat,
    config_hash: &str,
) -> String {
    format!(
        "/models/{}/exports/{}/{config_hash}/status",
        model.slug,
        format.slug()
    )
}

fn safe_filename_stem(value: &str) -> String {
    value
        .chars()
        .map(|character| match character {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' => character,
            _ => '-',
        })
        .collect()
}

fn configuration_hash(
    source_hash: &str,
    validated: &ValidatedConfiguration,
) -> anyhow::Result<String> {
    cache_model::config_hash(source_hash, SCHEMA_VERSION, &validated.typed_values)
}

fn config_values_json(values: &HashMap<String, String>) -> anyhow::Result<String> {
    Ok(serde_json::to_string(&cache_model::canonical_values(
        values,
    ))?)
}

fn typed_config_values_json(validated: &ValidatedConfiguration) -> anyhow::Result<String> {
    Ok(serde_json::to_string(&validated.typed_values)?)
}

fn configuration_validation_json(validated: &ValidatedConfiguration) -> anyhow::Result<String> {
    Ok(serde_json::to_string(&serde_json::json!({
        "parameterSchemaVersion": SCHEMA_VERSION,
        "requestValues": cache_model::canonical_values(&validated.values),
    }))?)
}

async fn persist_configuration_selection(
    state: &AppState,
    source_hash: &str,
    validated: &ValidatedConfiguration,
) -> anyhow::Result<String> {
    let config_hash = configuration_hash(source_hash, validated)?;
    let values_json = typed_config_values_json(validated)?;
    let validation_json = configuration_validation_json(validated)?;
    state
        .db
        .upsert_configuration_selection(db::ConfigurationSelectionUpsert {
            source_hash,
            config_hash: &config_hash,
            values_json: &values_json,
            validation_json: &validation_json,
        })
        .await?;
    Ok(config_hash)
}

async fn resolve_configuration_encoding(
    state: &AppState,
    model: &catalog::Model,
    source_hash: &str,
    config_hash: &str,
    values: &HashMap<String, String>,
) -> anyhow::Result<EncodedConfigurationIdentity> {
    if let Some(record) = state
        .db
        .configuration_encoding(source_hash, config_hash)
        .await?
    {
        return Ok(EncodedConfigurationIdentity {
            encoded_id: record.encoded_id,
            query_param: record.query_param,
        });
    }

    let encoded = state
        .onshape
        .encode_configuration(&model.onshape, values)
        .await?;
    state
        .db
        .upsert_configuration_encoding(db::ConfigurationEncodingUpsert {
            source_hash,
            config_hash,
            encoded_id: &encoded.identity.encoded_id,
            query_param: &encoded.identity.query_param,
            request_json: &encoded.request_json,
            response_json: &encoded.response_json,
        })
        .await?;
    Ok(encoded.identity)
}

fn parameter_schema_hash(schema: &ParameterSchema) -> anyhow::Result<String> {
    cache_model::parameter_schema_hash(schema)
}

async fn resolve_source_hash(state: &AppState, model: &catalog::Model) -> anyhow::Result<String> {
    let identity = resolve_source_identity(state, model).await?;
    cache_model::source_hash(&identity)
}

async fn resolve_source_identity(
    state: &AppState,
    model: &catalog::Model,
) -> anyhow::Result<ResolvedOnshapeSourceIdentity> {
    let source = &model.onshape;
    if let Some(record) = state
        .db
        .source_resolution_for_version(
            &source.document_id,
            &source.version_id,
            &source.element_id,
            source.element_kind.key(),
            source.link_document_id.as_deref(),
        )
        .await?
    {
        return Ok(ResolvedOnshapeSourceIdentity {
            document_id: record.document_id,
            version_id: record.version_id,
            microversion_id: record.microversion_id,
            element_id: record.element_id,
            element_kind: source.element_kind.clone(),
            link_document_id: record.link_document_id,
        });
    }

    let identity = state.onshape.resolve_version_microversion(source).await?;
    let source_hash = cache_model::source_hash(&identity)?;
    let diagnostics_json = serde_json::to_string(&serde_json::json!({
        "documentId": identity.document_id,
        "versionId": identity.version_id,
        "microversionId": identity.microversion_id,
    }))?;
    state
        .db
        .upsert_source_resolution(db::SourceResolutionUpsert {
            source_hash: &source_hash,
            model_slug: &model.slug,
            document_id: &identity.document_id,
            version_id: &identity.version_id,
            microversion_id: &identity.microversion_id,
            element_id: &identity.element_id,
            element_kind: identity.element_kind.key(),
            link_document_id: identity.link_document_id.as_deref(),
            diagnostics_json: &diagnostics_json,
        })
        .await?;
    Ok(identity)
}

fn preview_options_hash(model: &catalog::Model) -> String {
    options_hash(
        "glb",
        PREVIEW_OPTIONS_VERSION,
        &model.exports.preview_options,
    )
}

fn download_options_hash(model: &catalog::Model, format: catalog::DownloadFormat) -> String {
    let no_format_options = BTreeMap::<String, String>::new();
    match format {
        catalog::DownloadFormat::Step => options_hash(
            format.slug(),
            DOWNLOAD_OPTIONS_VERSION,
            &model.exports.download_options,
        ),
        catalog::DownloadFormat::Stl | catalog::DownloadFormat::ThreeMf => {
            options_hash(format.slug(), DOWNLOAD_OPTIONS_VERSION, &no_format_options)
        }
    }
}

fn options_hash<T>(format: &str, options_version: &'static str, options: &T) -> String
where
    T: Serialize,
{
    cache_model::options_hash(format, EXPORTER_VERSION, options_version, options)
        .expect("export options serialize")
}

fn work_key(kind: &'static str, payload: &WorkKeyPayload) -> String {
    format!(
        "work-v1:{kind}:{}",
        cache_key::hash_json("work-v1", payload).expect("work key payload serializes")
    )
}

fn download_artifact_id(
    source_hash: &str,
    config_hash: &str,
    options_hash: &str,
    format: catalog::DownloadFormat,
) -> String {
    cache_key::hash_json(
        "artifact-v1",
        &DownloadArtifactIdPayload {
            source_hash,
            config_hash,
            options_hash,
            format: format.slug(),
        },
    )
    .expect("download artifact identity serializes")
}

fn render_preview_result(state: &AppState, object_key: &str) -> String {
    render_preview_viewer(state, object_key)
}

fn render_clean_model_url_script(model: &catalog::Model) -> anyhow::Result<String> {
    let path = serde_json::to_string(&format!("/models/{}", model.slug))?;
    Ok(format!(
        r#"<script>
if (window.location.pathname !== {path}) {{
  window.history.replaceState(null, "", {path});
}}
</script>"#
    ))
}

fn render_preview_viewer(state: &AppState, object_key: &str) -> String {
    match state.storage.public_url(object_key) {
        Some(url) => format!(
            r#"<model-viewer src="{}" camera-controls auto-rotate environment-image="neutral" exposure="0.7" shadow-intensity="0.85" shadow-softness="0.6" style="width: min(100%, 720px); height: 480px; background: linear-gradient(#3b3f45, #25282d);"></model-viewer>"#,
            escape_html(&url),
        ),
        None => {
            "<p>Preview is cached, but TIGRIS_PUBLIC_BASE_URL is not configured.</p>".to_owned()
        }
    }
}

fn render_download_buttons(model: &catalog::Model) -> String {
    model
        .exports
        .downloads
        .iter()
        .map(|format| {
            format!(
                r#"<button type="submit" formaction="/models/{slug}/exports/{format_slug}">Generate {label}</button>"#,
                slug = escape_html(&model.slug),
                format_slug = format.slug(),
                label = format.label(),
            )
        })
        .collect::<String>()
}

fn render_download_prompt(model: &catalog::Model) -> String {
    if model.exports.downloads.is_empty() {
        "<p>This model does not expose downloadable formats.</p>".to_owned()
    } else {
        "<p>No cached downloads for these parameters yet.</p>".to_owned()
    }
}

fn render_download_result(
    state: &AppState,
    format: catalog::DownloadFormat,
    object_key: &str,
) -> String {
    format!(
        "{} export is ready. {}\n",
        format.label(),
        render_download_link(state, format, object_key)
    )
}

fn render_download_link(
    state: &AppState,
    format: catalog::DownloadFormat,
    object_key: &str,
) -> String {
    match state.storage.public_url(object_key) {
        Some(url) => format!(
            r#"<a href="{}">Download {}</a>"#,
            escape_html(&url),
            format.label()
        ),
        None => format!(
            "<span>{} export is cached, but TIGRIS_PUBLIC_BASE_URL is not configured.</span>",
            format.label()
        ),
    }
}

fn render_status_polling(
    kind: &str,
    config_hash: &str,
    status_url: &str,
    initial_message: &str,
) -> anyhow::Result<String> {
    let target_id = format!("status-{}-{}", safe_filename_stem(kind), &config_hash[..12]);
    let target_id_json = serde_json::to_string(&target_id)?;
    let kind_json = serde_json::to_string(kind)?;
    let status_url_json = serde_json::to_string(status_url)?;

    Ok(format!(
        r#"<p id="{target_id}">{initial_message} Status will update automatically.</p>
<script>
(() => {{
  const target = document.getElementById({target_id_json});
  const kind = {kind_json};
  const statusUrl = {status_url_json};
  const showReadyArtifact = (status) => {{
    if (!status.publicUrl) {{
      target.textContent = status.message;
      return;
    }}
    if (kind === "preview") {{
      const viewer = document.createElement("model-viewer");
      viewer.src = status.publicUrl;
      viewer.setAttribute("camera-controls", "");
      viewer.setAttribute("auto-rotate", "");
      viewer.style.width = "min(100%, 720px)";
      viewer.style.height = "480px";
      window.onshapeExportConfigurePreviewViewer?.(viewer);
      target.replaceWith(viewer);
      return;
    }}

    const link = document.createElement("a");
    link.href = status.publicUrl;
    link.textContent = `${{kind.toUpperCase()}} download is ready.`;
    target.replaceWith(link);
  }};
  const poll = async () => {{
    const response = await fetch(statusUrl);
    if (!response.ok) {{
      target.textContent = `Status check failed: ${{response.status}}`;
      return;
    }}
    const status = await response.json();
    target.textContent = status.errorSummary
      ? `${{status.message}} ${{status.errorSummary}}`
      : status.message;
    if (status.status === "ready") {{
      showReadyArtifact(status);
    }} else if (status.status !== "failed" && status.status !== "missing" && status.status !== "superseded") {{
      window.setTimeout(poll, 2000);
    }}
  }};
  window.setTimeout(poll, 1000);
}})();
</script>"#,
        target_id = escape_html(&target_id),
        initial_message = escape_html(initial_message),
        target_id_json = target_id_json,
        kind_json = kind_json,
        status_url_json = status_url_json,
    ))
}

fn render_parameter_controls(schema: &ParameterSchema) -> String {
    render_parameter_controls_with_values(schema, &HashMap::new())
}

fn render_parameter_controls_with_values(
    schema: &ParameterSchema,
    values: &HashMap<String, String>,
) -> String {
    if schema.parameters.is_empty() {
        return "<p>This model does not expose configurable parameters.</p>".to_owned();
    }

    let controls = schema
        .parameters
        .iter()
        .filter(|parameter| !parameter.hidden)
        .map(|parameter| {
            let id = escape_html(&parameter.id);
            let label = escape_html(&parameter.label);
            let display_value = values
                .get(&parameter.id)
                .cloned()
                .or_else(|| parameter.display_value())
                .unwrap_or_default();
            let required = if parameter.required { " required" } else { "" };
            let visibility_condition = parameter
                .visibility_condition
                .as_ref()
                .map(render_visibility_condition_attribute)
                .unwrap_or_default();
            let help = parameter
                .description
                .as_deref()
                .map(|description| format!(r#"<small>{}</small>"#, escape_html(description)))
                .unwrap_or_default();
            let input = match parameter.kind {
                ParameterKind::Text if parameter.widget.as_deref() == Some("textarea") => format!(
                    r#"<textarea id="{id}" name="{id}"{required}>{value}</textarea>"#,
                    value = escape_html(&display_value),
                ),
                ParameterKind::Text => {
                    format!(
                        r#"<input id="{id}" name="{id}" value="{value}"{required}>"#,
                        value = escape_html(&display_value),
                    )
                }
                ParameterKind::Number if parameter.units.is_some() => format!(
                    r#"<input id="{id}" name="{id}" value="{value}" inputmode="decimal"{required}>"#,
                    value = escape_html(&display_value),
                ),
                ParameterKind::Number => {
                    format!(
                        r#"<input id="{id}" name="{id}" type="{input_type}" step="{step}" value="{value}"{required}>"#,
                        input_type = if parameter.widget.as_deref() == Some("range") {
                            "range"
                        } else {
                            "number"
                        },
                        step = number_step(parameter.precision),
                        value = escape_html(&display_value),
                    )
                }
                ParameterKind::Boolean => {
                    let checked = matches!(display_value.as_str(), "true" | "on" | "1")
                        .then_some(" checked")
                        .unwrap_or("");
                    format!(
                        r#"<input type="hidden" name="{id}" value="false"><input id="{id}" name="{id}" type="checkbox" value="true"{checked}>"#
                    )
                }
                ParameterKind::Enum => {
                    let options = parameter
                        .options
                        .iter()
                        .map(|option| {
                            let selected = if option.value == display_value {
                                " selected"
                            } else {
                                ""
                            };
                            format!(
                                r#"<option value="{value}"{selected}>{label}</option>"#,
                                value = escape_html(&option.value),
                                label = escape_html(&option.label),
                            )
                        })
                        .collect::<String>();
                    format!(r#"<select id="{id}" name="{id}"{required}>{options}</select>"#)
                }
            };

            format!(
                r#"<p class="parameter-control" data-parameter-id="{id}"{visibility_condition}><label class="parameter-label" for="{id}">{label}</label><span class="parameter-value">{input}{help}</span></p>"#
            )
        })
        .collect::<String>();

    if controls.is_empty() {
        "<p>This model does not expose public configurable parameters.</p>".to_owned()
    } else {
        controls
    }
}

fn render_visibility_condition_attribute(condition: &ParameterVisibilityCondition) -> String {
    let json = serde_json::to_string(condition).expect("visibility condition serializes");
    format!(r#" data-visibility-condition="{}""#, escape_html(&json))
}

fn number_step(precision: Option<u32>) -> String {
    match precision {
        Some(0) => "1".to_owned(),
        Some(precision) => format!("0.{}1", "0".repeat(precision.saturating_sub(1) as usize)),
        None => "any".to_owned(),
    }
}

fn render_metrics(
    catalog_models: usize,
    job_metrics: &[db::JobMetric],
    artifact_metrics: &[db::ArtifactMetric],
) -> String {
    let mut output = String::from(
        "# HELP onshape_export_catalog_models Configured catalog models.\n\
# TYPE onshape_export_catalog_models gauge\n",
    );
    output.push_str(&format!(
        "onshape_export_catalog_models {}\n",
        catalog_models
    ));

    output.push_str(
        "# HELP onshape_export_jobs SQLite job rows by kind and status.\n\
# TYPE onshape_export_jobs gauge\n",
    );
    for metric in job_metrics {
        output.push_str(&format!(
            "onshape_export_jobs{{job_kind=\"{}\",status=\"{}\"}} {}\n",
            escape_metric_label(&metric.job_kind),
            escape_metric_label(&metric.status),
            metric.count
        ));
    }

    output.push_str(
        "# HELP onshape_export_artifacts SQLite artifact rows by output kind.\n\
# TYPE onshape_export_artifacts gauge\n",
    );
    for metric in artifact_metrics {
        output.push_str(&format!(
            "onshape_export_artifacts{{output_kind=\"{}\"}} {}\n",
            escape_metric_label(&metric.output_kind),
            metric.count
        ));
    }

    output.push_str(
        "# HELP onshape_export_artifact_bytes SQLite artifact bytes by output kind.\n\
# TYPE onshape_export_artifact_bytes gauge\n",
    );
    for metric in artifact_metrics {
        output.push_str(&format!(
            "onshape_export_artifact_bytes{{output_kind=\"{}\"}} {}\n",
            escape_metric_label(&metric.output_kind),
            metric.byte_len
        ));
    }

    output
}

fn escape_metric_label(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('\n', "\\n")
        .replace('"', "\\\"")
}

fn init_tracing() {
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .with(tracing_subscriber::fmt::layer())
        .init();
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn escapes_html() {
        assert_eq!(escape_html("<&>\"'"), "&lt;&amp;&gt;&quot;&#39;");
    }

    #[test]
    fn parameter_controls_preserve_submitted_values() {
        let schema = ParameterSchema {
            schema_version: parameters::SCHEMA_VERSION,
            source: test_model().onshape,
            parameters: vec![parameters::Parameter {
                id: "width".to_owned(),
                label: "Width".to_owned(),
                description: None,
                kind: ParameterKind::Number,
                required: false,
                default_value: Some("42".to_owned()),
                options: Vec::new(),
                hidden: false,
                visibility_condition: None,
                precision: None,
                widget: None,
                units: Some("millimeter".to_owned()),
                raw: Value::Null,
            }],
        };
        let values = HashMap::from([("width".to_owned(), "2 in".to_owned())]);

        let controls = render_parameter_controls_with_values(&schema, &values);

        assert!(controls.contains(r#"value="2 in""#));
        assert!(!controls.contains(r#"value="42 mm""#));
    }

    #[test]
    fn model_page_enhances_generation_submits() {
        let model = test_model();
        let html = render_model_html(&model, "", "", "").0;

        assert!(html.contains("model-layout"));
        assert!(html.contains("parameters-panel"));
        assert!(html.contains("output-panel"));
        assert!(html.contains("grid-template-columns: minmax(8rem, 42%) minmax(0, 1fr)"));
        assert!(html.contains("text-align: right"));
        assert!(html.contains("document.addEventListener(\"submit\""));
        assert!(html.contains("fetch(submitter.formAction"));
        assert!(html.contains("new URLSearchParams(new FormData(form))"));
        assert!(html.contains("application/x-www-form-urlencoded"));
        assert!(html.contains("window.onshapeExportConfigurePreviewViewer"));
        assert!(html.contains("pbr.setBaseColorFactor([0.48, 0.50, 0.52"));
        assert!(html.contains("replaceWith(nextMain)"));
        assert!(html.contains("initializeParameterVisibility(nextMain)"));
        assert!(html.contains(r#"form.addEventListener("change", update)"#));
        assert!(html.contains("wrapper.hidden = !evaluateVisibilityCondition"));
    }

    #[test]
    fn parameter_controls_render_visibility_metadata_without_disabling_inputs() {
        let schema = ParameterSchema {
            schema_version: parameters::SCHEMA_VERSION,
            source: test_model().onshape,
            parameters: vec![
                parameters::Parameter {
                    id: "dividers".to_owned(),
                    label: "Dividers".to_owned(),
                    description: None,
                    kind: ParameterKind::Boolean,
                    required: false,
                    default_value: Some("false".to_owned()),
                    options: Vec::new(),
                    hidden: false,
                    visibility_condition: None,
                    precision: None,
                    widget: None,
                    units: None,
                    raw: Value::Null,
                },
                parameters::Parameter {
                    id: "dividerCount".to_owned(),
                    label: "Divider Count".to_owned(),
                    description: None,
                    kind: ParameterKind::Number,
                    required: false,
                    default_value: Some("2".to_owned()),
                    options: Vec::new(),
                    hidden: false,
                    visibility_condition: Some(ParameterVisibilityCondition::Equal {
                        parameter_id: "dividers".to_owned(),
                        values: vec!["true".to_owned()],
                    }),
                    precision: Some(0),
                    widget: None,
                    units: None,
                    raw: Value::Null,
                },
            ],
        };

        let controls = render_parameter_controls(&schema);

        assert!(controls.contains(r#"class="parameter-control""#));
        assert!(controls.contains(r#"class="parameter-label""#));
        assert!(controls.contains(r#"class="parameter-value""#));
        assert!(controls.contains(r#"data-parameter-id="dividerCount""#));
        assert!(controls.contains(r#"data-visibility-condition="{&quot;kind&quot;:&quot;equal&quot;,&quot;parameterId&quot;:&quot;dividers&quot;,&quot;values&quot;:[&quot;true&quot;]}""#));
        assert!(controls.contains(r#"name="dividerCount""#));
        assert!(!controls.contains("disabled"));
    }

    #[test]
    fn status_polling_updates_preview_without_reloading() {
        let html = render_status_polling("preview", "abcdef123456", "/status", "Queued").unwrap();

        assert!(html.contains(r#"document.createElement("model-viewer")"#));
        assert!(html.contains("window.onshapeExportConfigurePreviewViewer?.(viewer)"));
        assert!(html.contains("showReadyArtifact(status)"));
        assert!(!html.contains("location.reload"));
    }

    #[test]
    fn hashes_configuration_canonically() {
        let first = HashMap::from([
            ("b".to_owned(), "2".to_owned()),
            ("a".to_owned(), "1".to_owned()),
        ]);
        let second = HashMap::from([
            ("a".to_owned(), "1".to_owned()),
            ("b".to_owned(), "2".to_owned()),
        ]);
        let model = test_model();
        let source_hash = resolved_source_hash_for_test_model(&model);
        let first_validated = validated_configuration_for_test_values(first);
        let second_validated = validated_configuration_for_test_values(second);

        assert_eq!(
            configuration_hash(&source_hash, &first_validated).unwrap(),
            configuration_hash(&source_hash, &second_validated).unwrap()
        );
    }

    #[test]
    fn config_hash_ignores_catalog_export_options() {
        let values = validated_configuration_for_test_values(HashMap::new());
        let first = test_model();
        let mut second = test_model();
        let first_source_hash = resolved_source_hash_for_test_model(&first);
        second.exports.preview_options.resolution = Some("FINE".to_owned());
        let second_source_hash = resolved_source_hash_for_test_model(&second);

        assert_eq!(
            configuration_hash(&first_source_hash, &values).unwrap(),
            configuration_hash(&second_source_hash, &values).unwrap()
        );
    }

    #[test]
    fn option_hashes_include_catalog_export_options() {
        let first = test_model();
        let mut second = test_model();
        second.exports.preview_options.resolution = Some("FINE".to_owned());

        assert_ne!(preview_options_hash(&first), preview_options_hash(&second));

        second.exports.download_options.step_version_string = Some("AP214".to_owned());
        assert_ne!(
            download_options_hash(&first, catalog::DownloadFormat::Step),
            download_options_hash(&second, catalog::DownloadFormat::Step)
        );
        assert_eq!(
            download_options_hash(&first, catalog::DownloadFormat::Stl),
            download_options_hash(&second, catalog::DownloadFormat::Stl)
        );
        assert_eq!(
            download_options_hash(&first, catalog::DownloadFormat::ThreeMf),
            download_options_hash(&second, catalog::DownloadFormat::ThreeMf)
        );
    }

    #[test]
    fn cache_keys_ignore_model_slug_and_public_filename() {
        let first = test_model();
        let mut second = test_model();
        second.slug = "other-slug".to_owned();
        let config_hash = "abc";
        let source_hash = resolved_source_hash_for_test_model(&first);
        let preview_options_hash = preview_options_hash(&first);
        let download_options_hash = download_options_hash(&first, catalog::DownloadFormat::Step);

        assert_eq!(
            configuration_hash(
                &source_hash,
                &validated_configuration_for_test_values(HashMap::new())
            )
            .unwrap(),
            configuration_hash(
                &source_hash,
                &validated_configuration_for_test_values(HashMap::new())
            )
            .unwrap()
        );
        assert_eq!(
            preview_artifact_key(&source_hash, &first, config_hash),
            preview_artifact_key(&source_hash, &second, config_hash)
        );
        assert_eq!(
            preview_work_key(&source_hash, &first, config_hash),
            preview_work_key(&source_hash, &second, config_hash)
        );
        assert_eq!(
            preview_glb_object_key(&source_hash, &first, config_hash),
            preview_glb_object_key(&source_hash, &second, config_hash)
        );
        assert_eq!(
            download_artifact_key(
                &source_hash,
                &first,
                config_hash,
                catalog::DownloadFormat::Step
            ),
            download_artifact_key(
                &source_hash,
                &second,
                config_hash,
                catalog::DownloadFormat::Step
            )
        );
        assert_eq!(
            download_object_key(
                &source_hash,
                &first,
                config_hash,
                catalog::DownloadFormat::Step
            ),
            download_object_key(
                &source_hash,
                &second,
                config_hash,
                catalog::DownloadFormat::Step
            )
        );
        assert_eq!(
            download_work_key(
                &source_hash,
                &first,
                config_hash,
                catalog::DownloadFormat::Step
            ),
            download_work_key(
                &source_hash,
                &second,
                config_hash,
                catalog::DownloadFormat::Step
            )
        );

        assert!(preview_artifact_key(&source_hash, &first, config_hash).contains(&source_hash));
        assert!(
            preview_artifact_key(&source_hash, &first, config_hash).contains(&preview_options_hash)
        );
        assert!(
            preview_glb_object_key(&source_hash, &first, config_hash)
                .contains(&preview_options_hash)
        );
        assert!(
            download_artifact_key(
                &source_hash,
                &first,
                config_hash,
                catalog::DownloadFormat::Step
            )
            .contains(&download_options_hash)
        );
        assert!(
            download_object_key(
                &source_hash,
                &first,
                config_hash,
                catalog::DownloadFormat::Step
            )
            .contains(&download_options_hash)
        );
        assert_ne!(
            preview_work_key(&source_hash, &first, config_hash),
            preview_artifact_key(&source_hash, &first, config_hash)
        );
        assert_ne!(
            download_filename(&first, catalog::DownloadFormat::Step),
            download_filename(&second, catalog::DownloadFormat::Step)
        );
    }

    #[test]
    fn rejects_parameter_overrides_unknown_to_schema() {
        let mut model = test_model();
        model
            .parameter_overrides
            .insert("missing".to_owned(), catalog::ParameterOverride::default());
        let schema = ParameterSchema {
            schema_version: parameters::SCHEMA_VERSION,
            source: model.onshape.clone(),
            parameters: vec![parameters::Parameter {
                id: "width".to_owned(),
                label: "Width".to_owned(),
                description: None,
                kind: ParameterKind::Number,
                required: false,
                default_value: Some("42".to_owned()),
                options: Vec::new(),
                hidden: false,
                visibility_condition: None,
                precision: None,
                widget: None,
                units: None,
                raw: Value::Null,
            }],
        };

        let error = validate_parameter_overrides(&model, &schema).unwrap_err();

        assert!(error.to_string().contains("unknown parameter: missing"));
    }

    #[test]
    fn preserves_direct_glb_preview_exports() {
        let model = test_model();
        let source_hash = resolved_source_hash_for_test_model(&model);
        let bytes = valid_glb();
        let artifact =
            preview_artifact_from_onshape_bytes(&source_hash, &model, "abc", bytes.clone())
                .unwrap();

        assert!(artifact.object_key.ends_with("preview.glb"));
        assert_eq!(artifact.content_type, "model/gltf-binary");
        assert_eq!(artifact.bytes, bytes);
    }

    #[test]
    fn preserves_direct_gltf_preview_exports() {
        let model = test_model();
        let source_hash = resolved_source_hash_for_test_model(&model);
        let bytes = br#"{"asset":{"version":"2.0"}}"#.to_vec();
        let artifact =
            preview_artifact_from_onshape_bytes(&source_hash, &model, "abc", bytes.clone())
                .unwrap();

        assert!(artifact.object_key.ends_with("preview.gltf"));
        assert_eq!(artifact.content_type, "model/gltf+json");
        assert_eq!(artifact.bytes, bytes);
        assert!(artifact.sidecars.is_empty());
    }

    #[test]
    fn extracts_single_glb_from_zipped_preview_exports() {
        let model = test_model();
        let source_hash = resolved_source_hash_for_test_model(&model);
        let glb = valid_glb();
        let bytes = test_zip(&[
            ("scene.gltf", br#"{"asset":{"version":"2.0"}}"#.as_slice()),
            ("scene.bin", b"loose buffer".as_slice()),
            ("preview.glb", glb.as_slice()),
        ]);

        let artifact =
            preview_artifact_from_onshape_bytes(&source_hash, &model, "abc", bytes).unwrap();

        assert!(artifact.object_key.ends_with("preview.glb"));
        assert_eq!(artifact.content_type, "model/gltf-binary");
        assert_eq!(artifact.bytes, glb);
        assert_eq!(artifact.sidecars.len(), 1);
        assert!(artifact.sidecars[0].object_key.ends_with("source.zip"));
        assert_eq!(artifact.sidecars[0].content_type, "application/zip");
    }

    #[test]
    fn rejects_invalid_direct_glb_preview_exports() {
        let model = test_model();
        let source_hash = resolved_source_hash_for_test_model(&model);

        let error =
            preview_artifact_from_onshape_bytes(&source_hash, &model, "abc", b"glTFbytes".to_vec())
                .unwrap_err();

        assert!(error.to_string().contains("validating direct GLB"));
    }

    #[test]
    fn extracts_gltf_asset_set_from_zipped_preview_exports() {
        let model = test_model();
        let source_hash = resolved_source_hash_for_test_model(&model);
        let bytes = test_zip(&[
            (
                "scene.gltf",
                br#"{"asset":{"version":"2.0"},"buffers":[{"uri":"scene.bin"}]}"#.as_slice(),
            ),
            ("scene.bin", b"loose buffer".as_slice()),
        ]);

        let artifact =
            preview_artifact_from_onshape_bytes(&source_hash, &model, "abc", bytes).unwrap();

        assert!(artifact.object_key.ends_with("scene.gltf"));
        assert_eq!(artifact.content_type, "model/gltf+json");
        assert_eq!(artifact.sidecars.len(), 2);
        assert!(
            artifact
                .sidecars
                .iter()
                .any(|sidecar| sidecar.object_key.ends_with("source.zip"))
        );
        assert!(
            artifact
                .sidecars
                .iter()
                .any(|sidecar| sidecar.object_key.ends_with("scene.bin"))
        );
    }

    #[test]
    fn rejects_multiple_gltf_zipped_preview_exports() {
        let model = test_model();
        let source_hash = resolved_source_hash_for_test_model(&model);
        let bytes = test_zip(&[
            ("first.gltf", br#"{"asset":{"version":"2.0"}}"#.as_slice()),
            ("second.gltf", br#"{"asset":{"version":"2.0"}}"#.as_slice()),
            ("scene.bin", b"loose buffer".as_slice()),
        ]);

        let error =
            preview_artifact_from_onshape_bytes(&source_hash, &model, "abc", bytes).unwrap_err();

        assert!(error.to_string().contains("multiple glTF files"));
    }

    #[test]
    fn rejects_unsafe_gltf_zip_asset_paths() {
        let model = test_model();
        let source_hash = resolved_source_hash_for_test_model(&model);
        let bytes = test_zip(&[
            ("scene.gltf", br#"{"asset":{"version":"2.0"}}"#.as_slice()),
            ("../scene.bin", b"loose buffer".as_slice()),
        ]);

        let error =
            preview_artifact_from_onshape_bytes(&source_hash, &model, "abc", bytes).unwrap_err();

        assert!(error.to_string().contains("not safe"));
    }

    #[test]
    fn rejects_multiple_glbs_in_zipped_preview_exports() {
        let model = test_model();
        let source_hash = resolved_source_hash_for_test_model(&model);
        let first = valid_glb();
        let second = valid_glb();
        let bytes = test_zip(&[
            ("first.glb", first.as_slice()),
            ("second.glb", second.as_slice()),
        ]);

        let error =
            preview_artifact_from_onshape_bytes(&source_hash, &model, "abc", bytes).unwrap_err();

        assert!(error.to_string().contains("multiple GLB files"));
    }

    #[test]
    fn rejects_invalid_zipped_glb_preview_exports() {
        let model = test_model();
        let source_hash = resolved_source_hash_for_test_model(&model);
        let bytes = test_zip(&[("preview.glb", b"glTFzip".as_slice())]);

        let error =
            preview_artifact_from_onshape_bytes(&source_hash, &model, "abc", bytes).unwrap_err();

        assert!(error.to_string().contains("validating zipped GLB"));
    }

    #[test]
    fn renders_prometheus_metrics() {
        let body = render_metrics(
            2,
            &[db::JobMetric {
                job_kind: "download_export".to_owned(),
                status: "ready".to_owned(),
                count: 3,
            }],
            &[db::ArtifactMetric {
                output_kind: "step".to_owned(),
                count: 3,
                byte_len: 42,
            }],
        );

        assert!(body.contains("onshape_export_catalog_models 2\n"));
        assert!(
            body.contains("onshape_export_jobs{job_kind=\"download_export\",status=\"ready\"} 3\n")
        );
        assert!(body.contains("onshape_export_artifact_bytes{output_kind=\"step\"} 42\n"));
    }

    #[test]
    fn builds_manifest_with_outputs_by_kind() {
        let model = test_model();
        let values = HashMap::from([("width".to_owned(), "10".to_owned())]);
        let artifacts = vec![db::ArtifactRecord {
            artifact_key: "download-step:model:key".to_owned(),
            model_slug: model.slug.clone(),
            config_hash: "abc".to_owned(),
            output_kind: "step".to_owned(),
            status: "ready".to_owned(),
            object_key: "artifacts/demo/file.step".to_owned(),
            content_type: "model/step".to_owned(),
            byte_len: Some(42),
            sha256: Some("abc123".to_owned()),
            producing_job_key: Some("work-v1:download:abc".to_owned()),
            source_hash: Some("sourcehash".to_owned()),
            options_hash: Some("optionshash".to_owned()),
            parameter_schema_version: Some(SCHEMA_VERSION.into()),
            config_values_json: Some(r#"{"width":"10"}"#.to_owned()),
            created_at: "2026-06-09T00:00:00.000Z".to_owned(),
            superseded_at: None,
        }];

        let manifest = build_manifest(
            "sourcehash",
            &model,
            "abc",
            Some(&values),
            &artifacts,
            Some("https://cdn.example.com/root"),
        );

        assert_eq!(manifest.manifest_schema_version, MANIFEST_SCHEMA_VERSION);
        assert_eq!(manifest.model_slug, "demo");
        assert_eq!(manifest.onshape.element_kind, "part_studio");
        assert_eq!(manifest.configuration.values["width"], "10");
        assert_eq!(
            manifest.outputs["step"].object_key,
            "artifacts/demo/file.step"
        );
        assert_eq!(manifest.outputs["step"].status, "ready");
        assert_eq!(manifest.outputs["step"].size_bytes, Some(42));
        assert_eq!(manifest.outputs["step"].sha256.as_deref(), Some("abc123"));
        assert_eq!(
            manifest.outputs["step"].job_id.as_deref(),
            Some("work-v1:download:abc")
        );
        assert_eq!(
            manifest.outputs["step"].public_url.as_deref(),
            Some("https://cdn.example.com/root/artifacts/demo/file.step")
        );
        assert_eq!(
            manifest.created_at.as_deref(),
            Some("2026-06-09T00:00:00.000Z")
        );

        let rewritten_manifest =
            build_manifest("sourcehash", &model, "abc", None, &artifacts, None);
        assert_eq!(rewritten_manifest.configuration.values["width"], "10");
    }

    #[test]
    fn public_urls_escape_object_key_segments() {
        assert_eq!(
            public_url_from_base("https://cdn.example.com/", "a b/file.step"),
            "https://cdn.example.com/a%20b/file.step"
        );
    }

    #[test]
    fn retry_backoff_uses_capped_full_jitter_windows() {
        for (attempt, max_delay) in [(1, 30), (2, 60), (5, 300), (20, 300)] {
            for _ in 0..50 {
                let delay = retry_backoff_seconds(attempt);
                assert!(delay >= 0);
                assert!(delay <= max_delay, "{delay} > {max_delay}");
            }
        }
    }

    #[test]
    fn escapes_metric_labels() {
        assert_eq!(escape_metric_label("a\\b\nc\"d"), "a\\\\b\\nc\\\"d");
    }

    #[test]
    fn parses_optional_output_format() {
        assert_eq!(optional_output_format(&[]).unwrap(), OutputFormat::Text);
        assert_eq!(
            optional_output_format(&["--json".to_owned()]).unwrap(),
            OutputFormat::Json
        );
        assert!(optional_output_format(&["--yaml".to_owned()]).is_err());
    }

    #[test]
    fn parses_optional_failure_retry_selector() {
        assert_eq!(
            optional_failure_retry_selector(&[]).unwrap(),
            FailureRetrySelector::All
        );
        assert_eq!(
            optional_failure_retry_selector(&["--all".to_owned()]).unwrap(),
            FailureRetrySelector::All
        );
        assert_eq!(
            optional_failure_retry_selector(&["work-v1:preview:demo:abc".to_owned()]).unwrap(),
            FailureRetrySelector::WorkKey("work-v1:preview:demo:abc")
        );
        assert_eq!(
            optional_failure_retry_selector(&["--kind".to_owned(), "preview_export".to_owned()])
                .unwrap(),
            FailureRetrySelector::Kind("preview_export")
        );
        assert!(optional_failure_retry_selector(&["--missing".to_owned()]).is_err());
        assert!(optional_failure_retry_selector(&["one".to_owned(), "two".to_owned()]).is_err());
    }

    #[test]
    fn parses_optional_parameter_selector() {
        assert_eq!(optional_parameter_selector(&[]).unwrap(), None);
        assert_eq!(
            optional_parameter_selector(&["small".to_owned()]).unwrap(),
            Some("small")
        );
        assert!(optional_parameter_selector(&["a".to_owned(), "b".to_owned()]).is_err());
    }

    #[test]
    fn parses_manifest_options() {
        assert_eq!(
            parse_manifest_options(&[]).unwrap(),
            ManifestOptions { rewrite: false }
        );
        assert_eq!(
            parse_manifest_options(&["--rewrite".to_owned()]).unwrap(),
            ManifestOptions { rewrite: true }
        );
        assert!(parse_manifest_options(&["--missing".to_owned()]).is_err());
    }

    fn test_model() -> catalog::Model {
        catalog::Model {
            catalog_schema_version: catalog::CATALOG_SCHEMA_VERSION,
            entry_version: catalog::CATALOG_ENTRY_VERSION,
            slug: "demo".to_owned(),
            name: "Demo".to_owned(),
            description: "Demo model".to_owned(),
            published: true,
            tags: Vec::new(),
            thumbnail: None,
            onshape: catalog::OnshapeSource {
                document_id: "did".to_owned(),
                version_id: "vid".to_owned(),
                element_id: "eid".to_owned(),
                element_kind: catalog::ElementKind::PartStudio,
                link_document_id: None,
            },
            exports: catalog::ExportConfig {
                downloads: vec![catalog::DownloadFormat::Step],
                preview: catalog::PreviewFormat::Glb,
                preview_options: catalog::PreviewOptions::default(),
                download_options: catalog::DownloadOptions::default(),
            },
            parameter_policy: catalog::ParameterPolicy {
                source: catalog::ParameterSource::Onshape,
                allow_unknown: false,
                auto_refresh: true,
            },
            parameter_presets: Vec::new(),
            parameter_overrides: HashMap::new(),
        }
    }

    fn resolved_source_hash_for_test_model(model: &catalog::Model) -> String {
        cache_model::source_hash(&ResolvedOnshapeSourceIdentity {
            document_id: model.onshape.document_id.clone(),
            version_id: model.onshape.version_id.clone(),
            microversion_id: "mid".to_owned(),
            element_id: model.onshape.element_id.clone(),
            element_kind: model.onshape.element_kind.clone(),
            link_document_id: model.onshape.link_document_id.clone(),
        })
        .unwrap()
    }

    fn validated_configuration_for_test_values(
        submitted: HashMap<String, String>,
    ) -> ValidatedConfiguration {
        let schema = ParameterSchema {
            schema_version: SCHEMA_VERSION,
            source: test_model().onshape,
            parameters: vec![
                parameters::Parameter {
                    id: "a".to_owned(),
                    label: "A".to_owned(),
                    description: None,
                    kind: ParameterKind::Number,
                    required: false,
                    default_value: None,
                    options: Vec::new(),
                    hidden: false,
                    visibility_condition: None,
                    precision: None,
                    widget: None,
                    units: None,
                    raw: Value::Null,
                },
                parameters::Parameter {
                    id: "b".to_owned(),
                    label: "B".to_owned(),
                    description: None,
                    kind: ParameterKind::Number,
                    required: false,
                    default_value: None,
                    options: Vec::new(),
                    hidden: false,
                    visibility_condition: None,
                    precision: None,
                    widget: None,
                    units: None,
                    raw: Value::Null,
                },
            ],
        };

        validate_values(&schema, &submitted, false).unwrap()
    }

    fn test_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut bytes = Cursor::new(Vec::new());
        {
            let mut writer = zip::ZipWriter::new(&mut bytes);
            let options = zip::write::SimpleFileOptions::default();
            for (name, contents) in entries {
                writer.start_file(name, options).unwrap();
                writer.write_all(contents).unwrap();
            }
            writer.finish().unwrap();
        }
        bytes.into_inner()
    }

    fn valid_glb() -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"glTF");
        bytes.extend_from_slice(&2_u32.to_le_bytes());
        bytes.extend_from_slice(&12_u32.to_le_bytes());
        bytes
    }
}
