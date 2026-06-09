mod catalog;
mod config;
mod db;
mod onshape;
mod parameters;
mod storage;

use std::{
    collections::{BTreeMap, HashMap},
    env,
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
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use tower_http::trace::TraceLayer;
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

use crate::parameters::{ParameterKind, ParameterSchema, normalize_configuration, validate_values};
use crate::{
    catalog::Catalog,
    config::Config,
    db::{ArtifactUpsert, Database},
    onshape::OnshapeClient,
    storage::StorageClient,
};

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
        values: HashMap<String, String>,
    },
    DownloadExport {
        model_slug: String,
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
    values: HashMap<String, String>,
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
    group_id: String,
    model_slug: String,
    element_kind: String,
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
    content_type: String,
    byte_len: Option<i64>,
    created_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ArtifactStatusResponse {
    artifact_key: String,
    status: String,
    message: String,
    public_url: Option<String>,
    error_summary: Option<String>,
    updated_at: Option<String>,
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
                        generate_preview_for_values(&state, model, &parameter_set.values).await?;
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
                            &parameter_set.values,
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
            match output_format {
                OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&jobs)?),
                OutputFormat::Text if jobs.is_empty() => println!("no failed jobs"),
                OutputFormat::Text => {
                    for job in jobs {
                        println!(
                            "{}\t{}\t{}\tattempt={}\tcreated={}\tupdated={}\t{}",
                            job.work_key,
                            job.job_kind,
                            job.status,
                            job.attempt,
                            job.created_at,
                            job.updated_at,
                            job.error_summary.unwrap_or_default()
                        );
                    }
                }
            }
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
                                "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                                artifact.artifact_key,
                                artifact.model_slug,
                                artifact.config_hash,
                                artifact.output_kind,
                                artifact.content_type,
                                artifact.byte_len.unwrap_or_default(),
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

            delete_artifact_and_rewrite_manifest(&state, &artifact).await?;
            println!(
                "invalidated artifact {artifact_key} and deleted {}",
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
            let manifest = build_manifest(model, config_hash, None, &artifacts);

            if options.rewrite {
                let object_key = manifest_object_key(model, config_hash);
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
                            "would prune"
                        } else {
                            "pruning"
                        },
                        artifact.artifact_key,
                        artifact.created_at,
                        artifact.byte_len.unwrap_or_default(),
                        artifact.object_key,
                    );
                    if !options.dry_run {
                        delete_artifact_and_rewrite_manifest(&state, &artifact).await?;
                    }
                    pruned += 1;
                }
            }

            println!(
                "{} {pruned} artifact(s) older than {} days",
                if options.dry_run { "matched" } else { "pruned" },
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
    values: &HashMap<String, String>,
) -> anyhow::Result<String> {
    let config_hash = configuration_hash(values)?;
    let artifact_key = preview_artifact_key(model, &config_hash);

    if let Some(record) = state.db.artifact(&artifact_key).await? {
        return Ok(record.object_key);
    }

    refresh_preview(state, model, values, &config_hash, &artifact_key).await
}

async fn enqueue_parameter_refresh(
    state: &AppState,
    model: &catalog::Model,
) -> anyhow::Result<bool> {
    let payload = JobPayload::ParameterRefresh {
        model_slug: model.slug.clone(),
    };
    enqueue_job(
        state,
        &format!("parameter-refresh:{}", model.slug),
        "parameter_refresh",
        &payload,
    )
    .await
}

async fn enqueue_preview(
    state: &AppState,
    model: &catalog::Model,
    values: &HashMap<String, String>,
) -> anyhow::Result<bool> {
    let config_hash = configuration_hash(values)?;
    let artifact_key = preview_artifact_key(model, &config_hash);
    let payload = JobPayload::PreviewGlb {
        model_slug: model.slug.clone(),
        values: values.clone(),
    };
    enqueue_job(state, &artifact_key, "preview_glb", &payload).await
}

async fn enqueue_download(
    state: &AppState,
    model: &catalog::Model,
    values: &HashMap<String, String>,
    format: catalog::DownloadFormat,
) -> anyhow::Result<bool> {
    let config_hash = configuration_hash(values)?;
    let artifact_key = download_artifact_key(model, &config_hash, format);
    let payload = JobPayload::DownloadExport {
        model_slug: model.slug.clone(),
        values: values.clone(),
        format,
    };
    enqueue_job(
        state,
        &artifact_key,
        &format!("export_{}", format.slug()),
        &payload,
    )
    .await
}

async fn enqueue_job(
    state: &AppState,
    work_key: &str,
    job_kind: &str,
    payload: &JobPayload,
) -> anyhow::Result<bool> {
    let payload_json = serde_json::to_string(payload)?;
    Ok(state
        .db
        .enqueue_job(work_key, job_kind, &payload_json)
        .await?)
}

async fn generate_download_for_values(
    state: &AppState,
    model: &catalog::Model,
    values: &HashMap<String, String>,
    format: catalog::DownloadFormat,
) -> anyhow::Result<Option<String>> {
    let config_hash = configuration_hash(values)?;
    let artifact_key = download_artifact_key(model, &config_hash, format);

    if let Some(record) = state.db.artifact(&artifact_key).await? {
        return Ok(Some(record.object_key));
    }

    refresh_download(state, model, values, &config_hash, &artifact_key, format)
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
        values: validated.values,
    })
}

fn print_usage() {
    eprintln!(
        "usage:\n  onshape-export [serve]\n  onshape-export worker\n  onshape-export catalog validate\n  onshape-export ops check\n  onshape-export parameters refresh <slug|--all>\n  onshape-export previews generate <slug|--all> [default|preset-slug|--all-parameter-sets]\n  onshape-export exports generate <slug|--all> <step|stl|3mf|--all> [default|preset-slug|--all-parameter-sets]\n  onshape-export failures list [--json]\n  onshape-export failures retry [--all|<work-key>|--kind <job-kind>]\n  onshape-export artifacts list <slug|--all> [--json]\n  onshape-export artifacts manifest <slug> <config-hash> [--rewrite]\n  onshape-export artifacts invalidate <artifact-key>\n  onshape-export artifacts prune <slug|--all> --older-than-days <days> [--dry-run]"
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
    let model = state.catalog.find(&slug).ok_or(AppError::NotFound)?;
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
</head>
<body>
  <main>
    <p><a href="/">Back to catalog</a></p>
    <h1>{name}</h1>
    <p>{description}</p>
    <form method="post">
      {parameter_controls}
      <button type="submit">Validate Parameters</button>
      <button type="submit" formaction="/models/{slug}/preview">Generate Preview</button>
      {download_buttons}
    </form>
    <section>
      <h2>Preview</h2>
      {preview}
    </section>
    <section>
      <h2>Downloads</h2>
      {downloads}
    </section>
  </main>
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
    let model = state.catalog.find(&slug).ok_or(AppError::NotFound)?;
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
    let model = state.catalog.find(&slug).ok_or(AppError::NotFound)?;
    let Some(parameters) = load_or_refresh_parameters(&state, model).await? else {
        return Ok(Html(
            "Parameter metadata is still refreshing. Try again shortly.\n".to_owned(),
        ));
    };
    let parameter_controls = render_parameter_controls(&parameters);
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
    let config_hash = configuration_hash(&validated.values)?;
    let artifact_key = preview_artifact_key(model, &config_hash);

    if let Some(record) = state.db.artifact(&artifact_key).await? {
        let preview = render_preview_result(&state, &record.object_key);
        let downloads = render_downloads_for_values(&state, model, &validated.values).await?;
        return Ok(render_model_html(
            model,
            &parameter_controls,
            &preview,
            &downloads,
        ));
    }

    enqueue_preview(&state, model, &validated.values).await?;
    let status_url = preview_status_path(model, &config_hash);
    Ok(render_model_html(
        model,
        &parameter_controls,
        &render_status_polling(
            "preview",
            &config_hash,
            &status_url,
            "Preview generation is queued.",
        )?,
        &render_download_prompt(model),
    ))
}

async fn generate_download(
    State(state): State<AppState>,
    Path((slug, format_slug)): Path<(String, String)>,
    Form(values): Form<HashMap<String, String>>,
) -> Result<Html<String>, AppError> {
    let model = state.catalog.find(&slug).ok_or(AppError::NotFound)?;
    let format = catalog::DownloadFormat::from_slug(&format_slug).ok_or(AppError::NotFound)?;
    if !model.exports.downloads.contains(&format) {
        return Err(AppError::NotFound);
    }

    let Some(parameters) = load_or_refresh_parameters(&state, model).await? else {
        return Ok(Html(
            "Parameter metadata is still refreshing. Try again shortly.\n".to_owned(),
        ));
    };
    let parameter_controls = render_parameter_controls(&parameters);
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
    let config_hash = configuration_hash(&validated.values)?;
    let artifact_key = download_artifact_key(model, &config_hash, format);

    if let Some(record) = state.db.artifact(&artifact_key).await? {
        let preview = render_preview_for_values(&state, model, &validated.values).await?;
        let downloads = render_download_result(&state, format, &record.object_key);
        return Ok(render_model_html(
            model,
            &parameter_controls,
            &preview,
            &downloads,
        ));
    }

    enqueue_download(&state, model, &validated.values, format).await?;
    let preview = render_preview_for_values(&state, model, &validated.values).await?;
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
    let model = state.catalog.find(&slug).ok_or(AppError::NotFound)?;
    let artifact_key = preview_artifact_key(model, &config_hash);

    Ok(Json(artifact_status(&state, &artifact_key).await?))
}

async fn download_status(
    State(state): State<AppState>,
    Path((slug, format_slug, config_hash)): Path<(String, String, String)>,
) -> Result<Json<ArtifactStatusResponse>, AppError> {
    let model = state.catalog.find(&slug).ok_or(AppError::NotFound)?;
    let format = catalog::DownloadFormat::from_slug(&format_slug).ok_or(AppError::NotFound)?;
    if !model.exports.downloads.contains(&format) {
        return Err(AppError::NotFound);
    }
    let artifact_key = download_artifact_key(model, &config_hash, format);

    Ok(Json(artifact_status(&state, &artifact_key).await?))
}

async fn artifact_status(
    state: &AppState,
    artifact_key: &str,
) -> Result<ArtifactStatusResponse, AppError> {
    if let Some(record) = state.db.artifact(artifact_key).await? {
        return Ok(ArtifactStatusResponse {
            artifact_key: artifact_key.to_owned(),
            status: "ready".to_owned(),
            message: "Artifact is ready.".to_owned(),
            public_url: state.storage.public_url(&record.object_key),
            error_summary: None,
            updated_at: Some(record.created_at),
        });
    }

    if let Some(job) = state.db.job(artifact_key).await? {
        let message = match job.status.as_str() {
            "queued" => "Generation is queued.",
            "running" => "Generation is running.",
            "failed" => "Generation failed.",
            "expired" => "Generation lease expired and will be retried.",
            "ready" => "Generation completed; artifact is not visible yet.",
            _ => "Generation status is unknown.",
        };
        return Ok(ArtifactStatusResponse {
            artifact_key: artifact_key.to_owned(),
            status: job.status,
            message: message.to_owned(),
            public_url: None,
            error_summary: job.error_summary,
            updated_at: Some(job.updated_at),
        });
    }

    Ok(ArtifactStatusResponse {
        artifact_key: artifact_key.to_owned(),
        status: "missing".to_owned(),
        message: "No cached artifact or queued generation was found.".to_owned(),
        public_url: None,
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
    if let Some(record) = state.db.parameter_metadata(&model.slug).await? {
        let schema = state
            .storage
            .get_json::<ParameterSchema>(&record.normalized_object_key)
            .await?;
        return Ok(Some(schema));
    }

    enqueue_parameter_refresh(state, model).await?;
    Ok(None)
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
        if enqueue_parameter_refresh(state, model).await? {
            enqueued += 1;
        }

        let Some(values) = cached_default_parameter_values(state, model).await? else {
            tracing::debug!(model = %model.slug, "default artifact rebuild skipped until parameter metadata is cached");
            continue;
        };

        let config_hash = configuration_hash(&values)?;
        let preview_artifact_key = preview_artifact_key(model, &config_hash);
        if state.db.artifact(&preview_artifact_key).await?.is_none()
            && enqueue_preview(state, model, &values).await?
        {
            enqueued += 1;
        }
        for format in &model.exports.downloads {
            let artifact_key = download_artifact_key(model, &config_hash, *format);
            if state.db.artifact(&artifact_key).await?.is_none()
                && enqueue_download(state, model, &values, *format).await?
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
) -> anyhow::Result<Option<HashMap<String, String>>> {
    let Some(record) = state.db.parameter_metadata(&model.slug).await? else {
        return Ok(None);
    };
    let schema = state
        .storage
        .get_json::<ParameterSchema>(&record.normalized_object_key)
        .await?;

    match validate_values(
        &schema,
        &HashMap::new(),
        model.parameter_policy.allow_unknown,
    ) {
        Ok(validated) => Ok(Some(validated.values)),
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
            if !state
                .db
                .finish_job(&job.work_key, job.attempt, "failed", Some(&summary))
                .await?
            {
                tracing::warn!(work_key = %job.work_key, attempt = job.attempt, "job lease was already reclaimed before failure was recorded");
            }
            return Err(error);
        }
    }

    Ok(true)
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
        JobPayload::PreviewGlb { model_slug, values } => {
            anyhow::ensure!(job.job_kind == "preview_glb", "unexpected preview job kind");
            let model = state
                .catalog
                .find(&model_slug)
                .ok_or_else(|| anyhow::anyhow!("unknown model slug: {model_slug}"))?;
            let config_hash = configuration_hash(&values)?;
            let artifact_key = preview_artifact_key(model, &config_hash);
            if state.db.artifact(&artifact_key).await?.is_none() {
                refresh_preview(state, model, &values, &config_hash, &artifact_key).await?;
            }
        }
        JobPayload::DownloadExport {
            model_slug,
            values,
            format,
        } => {
            anyhow::ensure!(
                job.job_kind == format!("export_{}", format.slug()),
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
            let config_hash = configuration_hash(&values)?;
            let artifact_key = download_artifact_key(model, &config_hash, format);
            if state.db.artifact(&artifact_key).await?.is_none() {
                refresh_download(state, model, &values, &config_hash, &artifact_key, format)
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
    let raw = state.onshape.fetch_configuration(&model.onshape).await?;
    let schema = normalize_configuration(&model.onshape, &raw);
    let raw_key = parameter_raw_key(model);
    let normalized_key = parameter_normalized_key(model);

    state.storage.put_json(&raw_key, &raw).await?;
    state.storage.put_json(&normalized_key, &schema).await?;
    state
        .db
        .upsert_parameter_metadata(&model.slug, &raw_key, &normalized_key)
        .await?;

    Ok(schema)
}

fn parameter_raw_key(model: &catalog::Model) -> String {
    format!(
        "onshape/{}/v/{}/e/{}/configuration.raw.json",
        model.onshape.document_id, model.onshape.version_id, model.onshape.element_id
    )
}

fn parameter_normalized_key(model: &catalog::Model) -> String {
    format!(
        "onshape/{}/v/{}/e/{}/parameters.normalized.json",
        model.onshape.document_id, model.onshape.version_id, model.onshape.element_id
    )
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
    let config_hash = configuration_hash(&validated.values)?;
    render_preview_for_hash(state, model, &config_hash, "default parameters").await
}

async fn render_preview_for_values(
    state: &AppState,
    model: &catalog::Model,
    values: &HashMap<String, String>,
) -> Result<String, AppError> {
    let config_hash = configuration_hash(values)?;
    render_preview_for_hash(state, model, &config_hash, "these parameters").await
}

async fn render_preview_for_hash(
    state: &AppState,
    model: &catalog::Model,
    config_hash: &str,
    label: &str,
) -> Result<String, AppError> {
    let artifact_key = preview_artifact_key(model, config_hash);

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

    render_downloads_for_values(state, model, &validated.values).await
}

async fn render_downloads_for_values(
    state: &AppState,
    model: &catalog::Model,
    values: &HashMap<String, String>,
) -> Result<String, AppError> {
    let config_hash = configuration_hash(values)?;
    let mut items = String::new();
    for format in &model.exports.downloads {
        let artifact_key = download_artifact_key(model, &config_hash, *format);
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
    values: &HashMap<String, String>,
    config_hash: &str,
    artifact_key: &str,
) -> anyhow::Result<String> {
    let configuration = onshape_configuration_string(values);
    let bytes = state
        .onshape
        .export_glb(&model.onshape, &configuration)
        .await?;
    let object_key = preview_object_key(model, config_hash);
    state
        .storage
        .put_bytes(&object_key, bytes.clone(), "model/gltf-binary")
        .await?;
    state
        .db
        .upsert_artifact(ArtifactUpsert {
            artifact_key,
            model_slug: &model.slug,
            config_hash,
            output_kind: "preview_glb",
            object_key: &object_key,
            content_type: "model/gltf-binary",
            byte_len: bytes.len() as i64,
        })
        .await?;
    rewrite_manifest(state, model, config_hash, Some(values)).await?;
    Ok(object_key)
}

async fn refresh_download(
    state: &AppState,
    model: &catalog::Model,
    values: &HashMap<String, String>,
    config_hash: &str,
    artifact_key: &str,
    format: catalog::DownloadFormat,
) -> anyhow::Result<String> {
    let configuration = onshape_configuration_string(values);
    let bytes = state
        .onshape
        .export_download(&model.onshape, &configuration, format)
        .await?;
    let object_key = download_object_key(model, config_hash, format);
    let filename = download_filename(model, format);
    let content_disposition = format!("attachment; filename=\"{filename}\"");
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
    let manifest = build_manifest(model, config_hash, values, &artifacts);
    let object_key = manifest_object_key(model, config_hash);
    state.storage.put_json(&object_key, &manifest).await?;
    Ok(object_key)
}

async fn delete_artifact_and_rewrite_manifest(
    state: &AppState,
    artifact: &db::ArtifactRecord,
) -> anyhow::Result<()> {
    state
        .storage
        .delete_object(&artifact.object_key)
        .await
        .with_context(|| format!("deleting object {}", artifact.object_key))?;
    state.db.delete_artifact(&artifact.artifact_key).await?;
    if let Some(model) = state.catalog.find(&artifact.model_slug) {
        rewrite_manifest(state, model, &artifact.config_hash, None).await?;
    }
    Ok(())
}

fn build_manifest(
    model: &catalog::Model,
    config_hash: &str,
    values: Option<&HashMap<String, String>>,
    artifacts: &[db::ArtifactRecord],
) -> ArtifactManifest {
    let outputs = artifacts
        .iter()
        .map(|artifact| {
            (
                artifact.output_kind.clone(),
                ManifestOutput {
                    artifact_key: artifact.artifact_key.clone(),
                    object_key: artifact.object_key.clone(),
                    content_type: artifact.content_type.clone(),
                    byte_len: artifact.byte_len,
                    created_at: artifact.created_at.clone(),
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
        group_id: manifest_group_id(model, config_hash),
        model_slug: model.slug.clone(),
        element_kind: element_kind_key(&model.onshape.element_kind).to_owned(),
        onshape: ManifestOnshapeSource {
            document_id: model.onshape.document_id.clone(),
            version_id: model.onshape.version_id.clone(),
            element_id: model.onshape.element_id.clone(),
        },
        configuration: ManifestConfiguration {
            hash: config_hash.to_owned(),
            values: values.map(canonical_values).unwrap_or_default(),
        },
        outputs,
        created_at,
        exporter_version: env!("CARGO_PKG_VERSION"),
    }
}

fn manifest_group_id(model: &catalog::Model, config_hash: &str) -> String {
    format!("{}:{}", source_identity(&model.onshape), config_hash)
}

fn manifest_object_key(model: &catalog::Model, config_hash: &str) -> String {
    format!(
        "manifests/{}/{}/{}/{}.json",
        model.slug, model.onshape.version_id, model.onshape.element_id, config_hash
    )
}

fn preview_artifact_key(model: &catalog::Model, config_hash: &str) -> String {
    format!(
        "preview-glb:{}:{}:{config_hash}:mesh-medium-v1",
        model.slug,
        source_identity(&model.onshape)
    )
}

fn preview_object_key(model: &catalog::Model, config_hash: &str) -> String {
    format!(
        "previews/{}/{}/{}/{}/mesh-medium-v1/preview.glb",
        model.slug, model.onshape.version_id, model.onshape.element_id, config_hash
    )
}

fn download_artifact_key(
    model: &catalog::Model,
    config_hash: &str,
    format: catalog::DownloadFormat,
) -> String {
    format!(
        "download-{}:{}:{}:{config_hash}:default-v1",
        format.slug(),
        model.slug,
        source_identity(&model.onshape)
    )
}

fn download_object_key(
    model: &catalog::Model,
    config_hash: &str,
    format: catalog::DownloadFormat,
) -> String {
    format!(
        "artifacts/{}/{}/{}/{}/{}/{}.{}",
        model.slug,
        model.onshape.version_id,
        model.onshape.element_id,
        config_hash,
        format.slug(),
        safe_filename_stem(&model.slug),
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

fn source_identity(source: &catalog::OnshapeSource) -> String {
    format!(
        "{}:{}:{}:{}",
        element_kind_key(&source.element_kind),
        source.document_id,
        source.version_id,
        source.element_id
    )
}

fn element_kind_key(kind: &catalog::ElementKind) -> &'static str {
    match kind {
        catalog::ElementKind::PartStudio => "part_studio",
        catalog::ElementKind::Assembly => "assembly",
    }
}

fn configuration_hash(values: &HashMap<String, String>) -> anyhow::Result<String> {
    let mut object = Map::new();
    for key in canonical_values(values).keys() {
        object.insert(key.clone(), Value::String(values[key].clone()));
    }
    let canonical = serde_json::to_vec(&Value::Object(object))?;
    Ok(hex_sha256(&canonical))
}

fn canonical_values(values: &HashMap<String, String>) -> BTreeMap<String, String> {
    values
        .iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

fn onshape_configuration_string(values: &HashMap<String, String>) -> String {
    let mut keys = values.keys().collect::<Vec<_>>();
    keys.sort();
    keys.into_iter()
        .map(|key| format!("{}={}", key, values[key]))
        .collect::<Vec<_>>()
        .join(";")
}

fn hex_sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn render_preview_result(state: &AppState, object_key: &str) -> String {
    format!(
        "Preview is ready. {}\n",
        render_preview_viewer(state, object_key)
    )
}

fn render_preview_viewer(state: &AppState, object_key: &str) -> String {
    match state.storage.public_url(object_key) {
        Some(url) => format!(
            r#"<model-viewer src="{}" camera-controls auto-rotate style="width: min(100%, 720px); height: 480px;"></model-viewer>"#,
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
    let status_url_json = serde_json::to_string(status_url)?;

    Ok(format!(
        r#"<p id="{target_id}">{initial_message} Status will update automatically.</p>
<script>
(() => {{
  const target = document.getElementById({target_id_json});
  const statusUrl = {status_url_json};
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
      target.textContent = "Artifact is ready. Updating page...";
      window.setTimeout(() => window.location.reload(), 500);
    }} else if (status.status !== "failed" && status.status !== "missing") {{
      window.setTimeout(poll, 2000);
    }}
  }};
  window.setTimeout(poll, 1000);
}})();
</script>"#,
        target_id = escape_html(&target_id),
        initial_message = escape_html(initial_message),
        target_id_json = target_id_json,
        status_url_json = status_url_json,
    ))
}

fn render_parameter_controls(schema: &ParameterSchema) -> String {
    if schema.parameters.is_empty() {
        return "<p>This model does not expose configurable parameters.</p>".to_owned();
    }

    schema
        .parameters
        .iter()
        .map(|parameter| {
            let id = escape_html(&parameter.id);
            let label = escape_html(&parameter.label);
            let default_value = parameter.default_value.as_deref().unwrap_or_default();
            let required = if parameter.required { " required" } else { "" };
            let input = match parameter.kind {
                ParameterKind::Text => format!(
                    r#"<input id="{id}" name="{id}" value="{value}"{required}>"#,
                    value = escape_html(default_value),
                ),
                ParameterKind::Number => format!(
                    r#"<input id="{id}" name="{id}" type="number" step="any" value="{value}"{required}>"#,
                    value = escape_html(default_value),
                ),
                ParameterKind::Boolean => {
                    let checked = matches!(default_value, "true" | "on" | "1")
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
                            let selected = if option.value == default_value {
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

            format!(r#"<p><label for="{id}">{label}</label><br>{input}</p>"#)
        })
        .collect()
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

    #[test]
    fn escapes_html() {
        assert_eq!(escape_html("<&>\"'"), "&lt;&amp;&gt;&quot;&#39;");
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

        assert_eq!(
            configuration_hash(&first).unwrap(),
            configuration_hash(&second).unwrap()
        );
    }

    #[test]
    fn renders_prometheus_metrics() {
        let body = render_metrics(
            2,
            &[db::JobMetric {
                job_kind: "export_step".to_owned(),
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
            body.contains("onshape_export_jobs{job_kind=\"export_step\",status=\"ready\"} 3\n")
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
            object_key: "artifacts/demo/file.step".to_owned(),
            content_type: "model/step".to_owned(),
            byte_len: Some(42),
            created_at: "2026-06-09T00:00:00.000Z".to_owned(),
        }];

        let manifest = build_manifest(&model, "abc", Some(&values), &artifacts);

        assert_eq!(manifest.model_slug, "demo");
        assert_eq!(manifest.configuration.values["width"], "10");
        assert_eq!(
            manifest.outputs["step"].object_key,
            "artifacts/demo/file.step"
        );
        assert_eq!(
            manifest.created_at.as_deref(),
            Some("2026-06-09T00:00:00.000Z")
        );
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
            optional_failure_retry_selector(&["preview_glb:demo:abc".to_owned()]).unwrap(),
            FailureRetrySelector::WorkKey("preview_glb:demo:abc")
        );
        assert_eq!(
            optional_failure_retry_selector(&["--kind".to_owned(), "preview_glb".to_owned()])
                .unwrap(),
            FailureRetrySelector::Kind("preview_glb")
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
            slug: "demo".to_owned(),
            name: "Demo".to_owned(),
            description: "Demo model".to_owned(),
            onshape: catalog::OnshapeSource {
                document_id: "did".to_owned(),
                version_id: "vid".to_owned(),
                element_id: "eid".to_owned(),
                element_kind: catalog::ElementKind::PartStudio,
            },
            exports: catalog::ExportConfig {
                downloads: vec![catalog::DownloadFormat::Step],
                preview: catalog::PreviewFormat::Glb,
            },
            parameter_policy: catalog::ParameterPolicy {
                source: catalog::ParameterSource::Onshape,
                allow_unknown: false,
            },
            parameter_presets: Vec::new(),
        }
    }
}
