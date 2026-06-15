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
    fs,
    io::{Cursor, Read},
    path::{Path as FsPath, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::Context;
use axum::{
    Form, Json, Router,
    extract::{Path, State},
    http::{StatusCode, header::CONTENT_TYPE},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
};
use clap::{Args, Parser, Subcommand};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tower_http::trace::TraceLayer;
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

use crate::parameters::{
    ParameterKind, ParameterSchema, ParameterVisibilityCondition, SCHEMA_VERSION,
    ValidatedConfiguration, apply_overrides, default_quantity_unit, encoding_request_values,
    normalize_configuration, quantity_unit_options, validate_values,
};
use crate::{
    cache_model::{EncodedConfigurationIdentity, ResolvedOnshapeSourceIdentity},
    catalog::Catalog,
    config::Config,
    db::{
        ArtifactUpsert, Database, ExportRequestInsert, RawPayloadInsert, RawPayloadSourceInsert,
        TranslationFinalUpdate, TranslationStartInsert,
    },
    onshape::OnshapeClient,
    storage::StorageClient,
};

const PREVIEW_OPTIONS_VERSION: &str = "mesh-grouped-v2";
const DOWNLOAD_OPTIONS_VERSION: &str = "default-v1";
const CONFIG_HASH_JOB_VERSION: u32 = 2;
const RETRY_BACKOFF_BASE_SECONDS: i64 = 30;
const RETRY_BACKOFF_CAP_SECONDS: i64 = 5 * 60;
const ALLOW_PARTIAL_MULTI_GLTF_PREVIEW_FALLBACK: bool = false;
const EXPORT_REQUEST_STATUS_STAGED: &str = "staged";
const POSTPROCESS_STATUS_STAGED: &str = "staged";
const POSTPROCESS_STATUS_READY: &str = "ready";
const POSTPROCESS_STATUS_FAILED: &str = "failed";
const PREVIEW_POSTPROCESSOR_NAME: &str = "preview_extract";
const PREVIEW_POSTPROCESSOR_VERSION: &str = "2";
const DOWNLOAD_POSTPROCESSOR_NAME: &str = "download_identity";
const DOWNLOAD_POSTPROCESSOR_VERSION: &str = "1";
const STRICT_UPLOAD_VERIFICATION: bool = false;
const V2_EXPORT_WORK_KEY_PREFIX: &str = "work-v2:export:";
const DEFAULT_CATALOG_SEED_PATH: &str = "catalog/v1/models.json";
const GENERATED_OBJECT_PREFIXES: &[&str] = &[
    "onshape/source/v2/",
    "onshape/raw/v2/",
    "previews/v2/",
    "artifacts/v2/",
];

#[derive(Debug, Parser)]
#[command(name = "onshape-export")]
struct Cli {
    #[command(subcommand)]
    command: Option<CliCommand>,
}

#[derive(Debug, Subcommand)]
#[command(rename_all = "kebab-case")]
enum CliCommand {
    Serve,
    Worker,
    Catalog {
        #[command(subcommand)]
        command: CatalogCommand,
    },
    Ops {
        #[command(subcommand)]
        command: OpsCommand,
    },
    Parameters {
        #[command(subcommand)]
        command: ParametersCommand,
    },
    Previews {
        #[command(subcommand)]
        command: PreviewsCommand,
    },
    Exports {
        #[command(subcommand)]
        command: ExportsCommand,
    },
    Jobs {
        #[command(subcommand)]
        command: JobsCommand,
    },
    Failures {
        #[command(subcommand)]
        command: FailuresCommand,
    },
    Artifacts {
        #[command(subcommand)]
        command: ArtifactsCommand,
    },
}

#[derive(Debug, Subcommand)]
#[command(rename_all = "kebab-case")]
enum CatalogCommand {
    Validate,
    Import { path: String },
    List(JsonOutputArgs),
    Show { slug: String },
}

#[derive(Debug, Subcommand)]
#[command(rename_all = "kebab-case")]
enum OpsCommand {
    Check,
    Backup { destination: PathBuf },
    DeployMaintenance(DeployMaintenanceArgs),
}

#[derive(Debug, Subcommand)]
#[command(rename_all = "kebab-case")]
enum ParametersCommand {
    Refresh(ModelSelectorArgs),
}

#[derive(Debug, Subcommand)]
#[command(rename_all = "kebab-case")]
enum PreviewsCommand {
    Generate(GeneratePreviewArgs),
}

#[derive(Debug, Subcommand)]
#[command(rename_all = "kebab-case")]
enum ExportsCommand {
    Generate(GenerateExportArgs),
}

#[derive(Debug, Subcommand)]
#[command(rename_all = "kebab-case")]
enum JobsCommand {
    List(JsonOutputArgs),
}

#[derive(Debug, Subcommand)]
#[command(rename_all = "kebab-case")]
enum FailuresCommand {
    List(JsonOutputArgs),
    Retry(FailureRetryArgs),
}

#[derive(Debug, Subcommand)]
#[command(rename_all = "kebab-case")]
enum ArtifactsCommand {
    List(ArtifactListArgs),
    Invalidate { artifact_key: String },
    Prune(PruneArgs),
}

#[derive(Debug, Args)]
struct JsonOutputArgs {
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct DeployMaintenanceArgs {
    #[arg(long)]
    reset_generated_state: bool,
    #[arg(long)]
    reset_catalog_from_seed: bool,
    #[arg(long)]
    fresh_database: bool,
    #[arg(long, default_value = DEFAULT_CATALOG_SEED_PATH)]
    catalog_seed: String,
    #[arg(long)]
    backup_label: Option<String>,
    #[arg(long, default_value = "sqlite")]
    backup_prefix: String,
    #[arg(long)]
    confirm: Option<String>,
}

#[derive(Debug, Args)]
struct ModelSelectorArgs {
    #[arg(allow_hyphen_values = true)]
    selector: String,
}

#[derive(Debug, Args)]
struct GeneratePreviewArgs {
    #[arg(allow_hyphen_values = true)]
    selector: String,
    #[arg(allow_hyphen_values = true)]
    parameter_selector: Option<String>,
}

#[derive(Debug, Args)]
struct GenerateExportArgs {
    #[arg(allow_hyphen_values = true)]
    selector: String,
    #[arg(allow_hyphen_values = true)]
    format: String,
    #[arg(allow_hyphen_values = true)]
    parameter_selector: Option<String>,
}

#[derive(Debug, Args)]
struct FailureRetryArgs {
    #[arg(long, conflicts_with_all = ["kind", "work_key"])]
    all: bool,
    #[arg(long, conflicts_with = "work_key")]
    kind: Option<String>,
    work_key: Option<String>,
}

#[derive(Debug, Args)]
struct ArtifactListArgs {
    #[arg(allow_hyphen_values = true)]
    selector: String,
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct PruneArgs {
    #[arg(allow_hyphen_values = true)]
    selector: String,
    #[arg(long, allow_hyphen_values = true)]
    older_than_days: i64,
    #[arg(long)]
    dry_run: bool,
}

#[derive(Clone)]
struct AppState {
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
        request_hash: String,
        #[serde(default)]
        source_hash: String,
        #[serde(default)]
        config_hash: String,
        #[serde(default)]
        options_hash: String,
        #[serde(default)]
        output_kind: String,
        #[serde(default)]
        format: String,
        #[serde(default)]
        config_hash_version: Option<u32>,
        values: HashMap<String, String>,
    },
    DownloadExport {
        model_slug: String,
        #[serde(default)]
        request_hash: String,
        #[serde(default)]
        source_hash: String,
        #[serde(default)]
        config_hash: String,
        #[serde(default)]
        options_hash: String,
        #[serde(default)]
        output_kind: String,
        #[serde(default)]
        export_format: String,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PruneOptions {
    older_than_days: i64,
    dry_run: bool,
}

#[derive(Debug, Clone)]
struct DeployMaintenanceOptions {
    reset_generated_state: bool,
    reset_catalog_from_seed: bool,
    fresh_database: bool,
    catalog_seed: String,
    backup_label: Option<String>,
    backup_prefix: String,
    confirm: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FailureRetrySelector<'a> {
    All,
    Kind(&'a str),
    WorkKey(&'a str),
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
    logical_path: String,
    original_path: Option<String>,
    content_type: &'static str,
    bytes: Vec<u8>,
    sidecars: Vec<PreviewAsset>,
}

#[derive(Debug)]
struct PreviewAsset {
    role: &'static str,
    logical_path: String,
    original_path: Option<String>,
    content_type: &'static str,
    bytes: Vec<u8>,
}

#[derive(Debug, Clone)]
struct PreparedExportRequest {
    options_hash: String,
    request_hash: String,
    request: onshape::CanonicalExportRequest,
}

#[derive(Debug)]
struct PersistedRawPayload {
    raw_payload_hash: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PreviewPostprocessPolicy {
    accepted_input_shapes: Vec<&'static str>,
    allow_partial_multi_gltf_preview_fallback: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DownloadPostprocessPolicy<'a> {
    strategy: &'static str,
    format: &'a str,
    content_type: &'a str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PostprocessLogEntry {
    level: &'static str,
    message: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DerivedArtifactFile<'a> {
    role: &'a str,
    logical_path: &'a str,
    original_path: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    object_key: Option<&'a str>,
    content_type: &'a str,
    byte_len: usize,
    sha256: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct RawZipEntry {
    path: String,
    byte_len: u64,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();

    let config = Config::from_env()?;
    let cli = Cli::parse();

    match cli.command {
        None | Some(CliCommand::Serve) => serve(config).await,
        Some(CliCommand::Worker) => run_worker(config).await,
        Some(command) => run_cli(config, command).await,
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

async fn run_cli(config: Config, command: CliCommand) -> anyhow::Result<()> {
    match command {
        CliCommand::Catalog {
            command: CatalogCommand::Validate,
        } => {
            let db = Database::connect(&config.database_url)
                .await
                .context("connecting to database")?;
            let catalog = db
                .catalog()
                .await
                .context("loading catalog from database")?;
            println!("catalog ok: {} models", catalog.models().len());
            Ok(())
        }
        CliCommand::Catalog {
            command: CatalogCommand::Import { path },
        } => {
            let catalog = Catalog::load(&path)
                .with_context(|| format!("loading catalog import from {path}"))?;
            let db = Database::connect(&config.database_url)
                .await
                .context("connecting to database")?;
            db.replace_catalog(&catalog)
                .await
                .context("importing catalog into database")?;
            println!("imported catalog: {} models", catalog.models().len());
            Ok(())
        }
        CliCommand::Catalog {
            command: CatalogCommand::List(output_args),
        } => {
            let output_format = output_args.output_format();
            let db = Database::connect(&config.database_url)
                .await
                .context("connecting to database")?;
            let catalog = db
                .catalog()
                .await
                .context("loading catalog from database")?;
            match output_format {
                OutputFormat::Json => {
                    println!("{}", serde_json::to_string_pretty(catalog.models())?)
                }
                OutputFormat::Text if catalog.models().is_empty() => println!("no catalog models"),
                OutputFormat::Text => {
                    for model in catalog.models() {
                        println!(
                            "{}\t{}\t{}\t{}",
                            model.slug,
                            if model.published {
                                "published"
                            } else {
                                "draft"
                            },
                            model.name,
                            model.description
                        );
                    }
                }
            }
            Ok(())
        }
        CliCommand::Catalog {
            command: CatalogCommand::Show { slug },
        } => {
            let db = Database::connect(&config.database_url)
                .await
                .context("connecting to database")?;
            let catalog = db
                .catalog()
                .await
                .context("loading catalog from database")?;
            let model = catalog
                .find(&slug)
                .ok_or_else(|| anyhow::anyhow!("unknown model slug: {slug}"))?;
            println!("{}", serde_json::to_string_pretty(model)?);
            Ok(())
        }
        CliCommand::Ops {
            command: OpsCommand::Check,
        } => run_ops_check(config).await,
        CliCommand::Ops {
            command: OpsCommand::DeployMaintenance(maintenance_args),
        } => run_deploy_maintenance(config, maintenance_args.into()).await,
        CliCommand::Ops {
            command: OpsCommand::Backup { destination },
        } => {
            let db = Database::connect(&config.database_url)
                .await
                .context("connecting to database")?;
            db.backup_to_path(&destination).await?;
            println!("database backup written to {}", destination.display());
            Ok(())
        }
        CliCommand::Parameters {
            command: ParametersCommand::Refresh(ModelSelectorArgs { selector }),
        } => {
            let state = cli_state(config).await?;
            let catalog = state
                .db
                .catalog()
                .await
                .context("loading catalog from database")?;
            for model in selected_models(&catalog, &selector)? {
                refresh_parameters(&state, model).await?;
                println!("refreshed parameters for {}", model.slug);
            }
            Ok(())
        }
        CliCommand::Previews {
            command:
                PreviewsCommand::Generate(GeneratePreviewArgs {
                    selector,
                    parameter_selector,
                }),
        } => {
            let state = cli_state(config).await?;
            let catalog = state
                .db
                .catalog()
                .await
                .context("loading catalog from database")?;
            for model in selected_models(&catalog, &selector)? {
                for parameter_set in
                    selected_parameter_sets(&state, model, parameter_selector.as_deref()).await?
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
        CliCommand::Exports {
            command:
                ExportsCommand::Generate(GenerateExportArgs {
                    selector,
                    format,
                    parameter_selector,
                }),
        } => {
            let state = cli_state(config).await?;
            let catalog = state
                .db
                .catalog()
                .await
                .context("loading catalog from database")?;
            for model in selected_models(&catalog, &selector)? {
                let formats = selected_formats(model, &format)?;
                for parameter_set in
                    selected_parameter_sets(&state, model, parameter_selector.as_deref()).await?
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
        CliCommand::Failures {
            command: FailuresCommand::List(output_args),
        } => {
            let output_format = output_args.output_format();
            let state = cli_state(config).await?;
            let jobs = state.db.failed_jobs(100).await?;
            print_jobs(jobs, output_format, "no failed jobs")?;
            Ok(())
        }
        CliCommand::Jobs {
            command: JobsCommand::List(output_args),
        } => {
            let output_format = output_args.output_format();
            let state = cli_state(config).await?;
            let jobs = state.db.jobs(100).await?;
            print_jobs(jobs, output_format, "no jobs")?;
            Ok(())
        }
        CliCommand::Failures {
            command: FailuresCommand::Retry(retry_args),
        } => {
            let state = cli_state(config).await?;
            match retry_args.selector() {
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
        CliCommand::Artifacts {
            command: ArtifactsCommand::List(ArtifactListArgs { selector, json }),
        } => {
            let output_format = output_format(json);
            let state = cli_state(config).await?;
            let mut all_artifacts = Vec::new();
            let catalog = state
                .db
                .catalog()
                .await
                .context("loading catalog from database")?;
            for model in selected_models(&catalog, &selector)? {
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
        CliCommand::Artifacts {
            command: ArtifactsCommand::Invalidate { artifact_key },
        } => {
            let state = cli_state(config).await?;
            let Some(artifact) = state.db.artifact(&artifact_key).await? else {
                println!("artifact not found: {artifact_key}");
                return Ok(());
            };

            supersede_published_artifact(&state, &artifact).await?;
            println!(
                "invalidated artifact {artifact_key} and marked {} superseded",
                artifact.object_key
            );
            Ok(())
        }
        CliCommand::Artifacts {
            command:
                ArtifactsCommand::Prune(PruneArgs {
                    selector,
                    older_than_days,
                    dry_run,
                }),
        } => {
            let options = PruneOptions::new(older_than_days, dry_run)?;
            let state = cli_state(config).await?;
            let mut pruned = 0usize;

            let catalog = state
                .db
                .catalog()
                .await
                .context("loading catalog from database")?;
            for model in selected_models(&catalog, &selector)? {
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
                        supersede_published_artifact(&state, &artifact).await?;
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
        CliCommand::Serve | CliCommand::Worker => unreachable!("handled before run_cli"),
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

impl JsonOutputArgs {
    fn output_format(&self) -> OutputFormat {
        output_format(self.json)
    }
}

fn output_format(json: bool) -> OutputFormat {
    if json {
        OutputFormat::Json
    } else {
        OutputFormat::Text
    }
}

impl From<DeployMaintenanceArgs> for DeployMaintenanceOptions {
    fn from(args: DeployMaintenanceArgs) -> Self {
        Self {
            reset_generated_state: args.reset_generated_state,
            reset_catalog_from_seed: args.reset_catalog_from_seed,
            fresh_database: args.fresh_database,
            catalog_seed: args.catalog_seed,
            backup_label: args.backup_label,
            backup_prefix: args.backup_prefix,
            confirm: args.confirm,
        }
    }
}

impl FailureRetryArgs {
    fn selector(&self) -> FailureRetrySelector<'_> {
        if let Some(job_kind) = self.kind.as_deref() {
            FailureRetrySelector::Kind(job_kind)
        } else if let Some(work_key) = self.work_key.as_deref() {
            FailureRetrySelector::WorkKey(work_key)
        } else {
            FailureRetrySelector::All
        }
    }
}

impl PruneOptions {
    fn new(older_than_days: i64, dry_run: bool) -> anyhow::Result<Self> {
        anyhow::ensure!(
            older_than_days > 0,
            "--older-than-days must be greater than zero"
        );
        Ok(Self {
            older_than_days,
            dry_run,
        })
    }
}

async fn run_ops_check(config: Config) -> anyhow::Result<()> {
    let mut failures = Vec::new();

    match Database::connect(&config.database_url).await {
        Ok(db) => {
            match db.ping().await {
                Ok(()) => println!("database ok: {}", config.database_url),
                Err(error) => failures.push(format!("database ping failed: {error:#}")),
            }
            match db.catalog().await {
                Ok(catalog) if catalog.models().is_empty() => {
                    failures.push("catalog load failed: catalog is empty".to_owned())
                }
                Ok(catalog) => println!("catalog ok: {} models", catalog.models().len()),
                Err(error) => failures.push(format!("catalog load failed: {error:#}")),
            }
        }
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

async fn run_deploy_maintenance(
    config: Config,
    options: DeployMaintenanceOptions,
) -> anyhow::Result<()> {
    ensure_destructive_options_confirmed(&options)?;
    let database_path = sqlite_database_path(&config.database_url).with_context(|| {
        format!(
            "unsupported DATABASE_URL for deploy maintenance: {}",
            config.database_url
        )
    })?;

    backup_database_to_private_storage(&config, &options, &database_path).await?;

    if options.fresh_database {
        remove_sqlite_database_files(&database_path)?;
        println!(
            "deleted sqlite database files for {}",
            database_path.display()
        );
    }

    let db = Database::connect(&config.database_url)
        .await
        .context("connecting to database after maintenance preparation")?;

    if options.fresh_database || options.reset_catalog_from_seed {
        import_catalog_seed(&db, &options.catalog_seed).await?;
    }

    if options.reset_generated_state {
        let storage = StorageClient::new(config.storage.clone()).await?;
        reset_generated_objects(&storage).await?;
        let deleted = db.clear_generated_state().await?;
        for table in deleted {
            println!("deleted {} row(s) from {}", table.rows, table.table);
        }
    }

    ensure_database_ready_for_serve(&db).await?;
    println!("deploy maintenance ok");
    Ok(())
}

fn ensure_destructive_options_confirmed(options: &DeployMaintenanceOptions) -> anyhow::Result<()> {
    if options.reset_generated_state || options.reset_catalog_from_seed || options.fresh_database {
        anyhow::ensure!(
            options.confirm.as_deref() == Some("WIPE"),
            "destructive deploy maintenance options require --confirm WIPE"
        );
    }
    Ok(())
}

async fn backup_database_to_private_storage(
    config: &Config,
    options: &DeployMaintenanceOptions,
    database_path: &FsPath,
) -> anyhow::Result<Option<String>> {
    let Some(backup_storage_config) = config.backup_storage.clone() else {
        anyhow::bail!("BACKUP_TIGRIS_BUCKET is required for deploy maintenance backups");
    };
    anyhow::ensure!(
        backup_storage_config.access_key_id.is_some()
            && backup_storage_config.secret_access_key.is_some(),
        "backup storage credentials are incomplete; set BACKUP_AWS_ACCESS_KEY_ID and BACKUP_AWS_SECRET_ACCESS_KEY"
    );

    if !database_path.exists() {
        println!(
            "database file does not exist yet; no sqlite backup was uploaded for {}",
            database_path.display()
        );
        return Ok(None);
    }

    let label = options
        .backup_label
        .clone()
        .unwrap_or_else(default_backup_label);
    let label = safe_backup_label(&label)?;
    let prefix = options.backup_prefix.trim_matches('/');
    let backup_key = if prefix.is_empty() {
        format!("{label}.db")
    } else {
        format!("{prefix}/{label}.db")
    };
    let backup_dir = tempfile::Builder::new()
        .prefix("onshape-export-")
        .tempdir()
        .context("creating temporary sqlite backup directory")?;
    let backup_path = backup_dir.path().join("backup.db");

    let db = Database::connect_without_migrations(&config.database_url)
        .await
        .context("connecting to database for pre-deploy backup")?;
    db.backup_to_path(&backup_path).await?;
    drop(db);

    let backup_storage = StorageClient::new(backup_storage_config).await?;
    let upload_result = backup_storage
        .put_file(&backup_key, &backup_path, "application/vnd.sqlite3")
        .await
        .with_context(|| format!("uploading sqlite backup to {backup_key}"));
    upload_result?;
    println!("uploaded sqlite backup to {backup_key}");
    Ok(Some(backup_key))
}

fn default_backup_label() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("manual-{seconds}")
}

fn safe_backup_label(label: &str) -> anyhow::Result<String> {
    let trimmed = label.trim();
    anyhow::ensure!(!trimmed.is_empty(), "backup label cannot be empty");
    anyhow::ensure!(
        trimmed
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.')),
        "backup label may only contain ASCII letters, numbers, dots, underscores, and hyphens"
    );
    Ok(trimmed.to_owned())
}

fn sqlite_database_path(database_url: &str) -> Option<PathBuf> {
    let database_url = database_url
        .split_once('?')
        .map_or(database_url, |(path, _)| path);
    if database_url == "sqlite::memory:" || database_url.ends_with(":memory:") {
        return None;
    }

    database_url
        .strip_prefix("sqlite://")
        .or_else(|| database_url.strip_prefix("sqlite:"))
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
}

fn remove_sqlite_database_files(database_path: &FsPath) -> anyhow::Result<()> {
    for path in sqlite_database_files(database_path) {
        match fs::remove_file(&path) {
            Ok(()) => println!("removed {}", path.display()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("removing sqlite file {}", path.display()));
            }
        }
    }
    Ok(())
}

fn sqlite_database_files(database_path: &FsPath) -> Vec<PathBuf> {
    let mut paths = Vec::with_capacity(3);
    paths.push(database_path.to_owned());
    let path = database_path.as_os_str().to_string_lossy();
    paths.push(PathBuf::from(format!("{path}-wal")));
    paths.push(PathBuf::from(format!("{path}-shm")));
    paths
}

async fn import_catalog_seed(db: &Database, catalog_seed: &str) -> anyhow::Result<()> {
    let catalog = Catalog::load(catalog_seed)
        .with_context(|| format!("loading catalog seed from {catalog_seed}"))?;
    db.replace_catalog(&catalog)
        .await
        .with_context(|| format!("importing catalog seed from {catalog_seed}"))?;
    println!("imported catalog seed: {} models", catalog.models().len());
    Ok(())
}

async fn reset_generated_objects(storage: &StorageClient) -> anyhow::Result<()> {
    for prefix in GENERATED_OBJECT_PREFIXES {
        let summary = storage
            .delete_prefix(prefix)
            .await
            .with_context(|| format!("deleting generated objects under {prefix}"))?;
        println!("deleted {} object(s) under {prefix}", summary.objects);
    }
    Ok(())
}

async fn ensure_database_ready_for_serve(db: &Database) -> anyhow::Result<()> {
    db.ping().await?;
    let catalog = db.catalog().await?;
    anyhow::ensure!(
        !catalog.models().is_empty(),
        "catalog is empty after deploy maintenance"
    );
    println!("maintenance catalog ok: {} models", catalog.models().len());
    Ok(())
}

async fn cli_state(config: Config) -> anyhow::Result<AppState> {
    build_state(config).await
}

async fn build_state(config: Config) -> anyhow::Result<AppState> {
    let db = Database::connect(&config.database_url)
        .await
        .context("connecting to database")?;
    db.catalog()
        .await
        .context("loading catalog from database")?;
    let storage = StorageClient::new(config.storage.clone()).await?;
    let onshape = OnshapeClient::new(config.onshape.clone())?;

    Ok(AppState {
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
    let prepared =
        prepare_preview_export(state, model, &source_hash, validated, &config_hash).await?;

    if let Some(record) = ready_preview_artifact(state, &prepared.request_hash).await? {
        return Ok(record.object_key);
    }

    refresh_preview(
        state,
        model,
        &source_hash,
        validated,
        &config_hash,
        Some(prepared),
        None,
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
    let prepared =
        prepare_preview_export(state, model, &source_hash, validated, &config_hash).await?;
    let payload = JobPayload::PreviewGlb {
        model_slug: model.slug.clone(),
        request_hash: prepared.request_hash.clone(),
        source_hash: source_hash.clone(),
        config_hash: config_hash.clone(),
        options_hash: prepared.options_hash,
        output_kind: "preview".to_owned(),
        format: "glb".to_owned(),
        config_hash_version: Some(CONFIG_HASH_JOB_VERSION),
        values: validated.values.clone(),
    };
    enqueue_job(
        state,
        &export_job_key(&prepared.request_hash),
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
    let prepared =
        prepare_download_export(state, model, &source_hash, validated, &config_hash, format)
            .await?;
    let payload = JobPayload::DownloadExport {
        model_slug: model.slug.clone(),
        request_hash: prepared.request_hash.clone(),
        source_hash: source_hash.clone(),
        config_hash: config_hash.clone(),
        options_hash: prepared.options_hash,
        output_kind: "download".to_owned(),
        export_format: format.slug().to_owned(),
        config_hash_version: Some(CONFIG_HASH_JOB_VERSION),
        values: validated.values.clone(),
        format,
    };
    enqueue_job(
        state,
        &export_job_key(&prepared.request_hash),
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
    let prepared =
        prepare_download_export(state, model, &source_hash, validated, &config_hash, format)
            .await?;

    if let Some(record) = ready_download_artifact(state, &prepared.request_hash).await? {
        return Ok(Some(record.object_key));
    }

    refresh_download(
        state,
        model,
        &source_hash,
        validated,
        &config_hash,
        format,
        Some(prepared),
        None,
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
    let catalog = state.db.catalog().await?;
    let job_metrics = state.db.job_metrics().await?;
    let artifact_metrics = state.db.artifact_metrics().await?;
    let body = render_metrics(catalog.models().len(), &job_metrics, &artifact_metrics);

    Ok(([(CONTENT_TYPE, "text/plain; version=0.0.4")], body))
}

async fn index(State(state): State<AppState>) -> Result<Html<String>, AppError> {
    let catalog = state.db.catalog().await?;
    let models = catalog
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

    Ok(Html(format!(
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
    )))
}

async fn model_page(
    State(state): State<AppState>,
    Path(slug): Path<String>,
) -> Result<Html<String>, AppError> {
    let catalog = state.db.catalog().await?;
    let model = published_model(&catalog, &slug).ok_or(AppError::NotFound)?;
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
    .quantity-control {{
      display: grid;
      grid-template-columns: minmax(0, 1fr) minmax(5rem, auto);
      gap: 0.5rem;
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
      .quantity-control {{
        grid-template-columns: 1fr;
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
    const formControls = Array.from(form.elements);
    const controls = formControls.filter((control) => control.name === parameterId);
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
    if (control) {{
      return control.value;
    }}

    const quantityValueControl = formControls.find(
      (control) => control.name === `${{parameterId}}__value`
    );
    if (!quantityValueControl) {{
      return undefined;
    }}
    const value = quantityValueControl.value.trim();
    if (!value) {{
      return "";
    }}
    const quantityUnitControl = formControls.find(
      (control) => control.name === `${{parameterId}}__unit`
    );
    const unit = quantityUnitControl?.value.trim() ?? "";
    return unit ? `${{value}} ${{unit}}` : value;
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
    let catalog = state.db.catalog().await?;
    let model = published_model(&catalog, &slug).ok_or(AppError::NotFound)?;
    let Some(parameters) = load_or_refresh_parameters(&state, model).await? else {
        return Ok(Html(
            "Parameter metadata is still refreshing. Try again shortly.\n".to_owned(),
        ));
    };
    let values = normalize_form_values(&parameters, values);

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
    let catalog = state.db.catalog().await?;
    let model = published_model(&catalog, &slug).ok_or(AppError::NotFound)?;
    let Some(parameters) = load_or_refresh_parameters(&state, model).await? else {
        return Ok(Html(
            "Parameter metadata is still refreshing. Try again shortly.\n".to_owned(),
        ));
    };
    let values = normalize_form_values(&parameters, values);
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
    let prepared =
        prepare_preview_export(&state, model, &source_hash, &validated, &config_hash).await?;

    if let Some(record) = ready_preview_artifact(&state, &prepared.request_hash).await? {
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
    let catalog = state.db.catalog().await?;
    let model = published_model(&catalog, &slug).ok_or(AppError::NotFound)?;
    let format = catalog::DownloadFormat::from_slug(&format_slug).ok_or(AppError::NotFound)?;
    if !model.exports.downloads.contains(&format) {
        return Err(AppError::NotFound);
    }

    let Some(parameters) = load_or_refresh_parameters(&state, model).await? else {
        return Ok(Html(
            "Parameter metadata is still refreshing. Try again shortly.\n".to_owned(),
        ));
    };
    let values = normalize_form_values(&parameters, values);
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
    let prepared = prepare_download_export(
        &state,
        model,
        &source_hash,
        &validated,
        &config_hash,
        format,
    )
    .await?;

    if let Some(record) = ready_download_artifact(&state, &prepared.request_hash).await? {
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
    let model = state
        .db
        .published_catalog_model(&slug)
        .await?
        .ok_or(AppError::NotFound)?;
    let source_hash = resolve_source_hash(&state, &model).await?;
    let request_hash =
        current_preview_request_hash(&state, &model, &source_hash, &config_hash).await?;
    let work_keys = request_hash
        .as_deref()
        .map(|request_hash| vec![export_job_key(request_hash)])
        .unwrap_or_default();
    let artifact = if let Some(request_hash) = request_hash.as_deref() {
        ready_preview_artifact(&state, request_hash).await?
    } else {
        None
    };
    let artifact_set = if let Some(request_hash) = request_hash.as_deref() {
        latest_artifact_set(&state, request_hash).await?
    } else {
        None
    };

    Ok(Json(
        artifact_status(
            &state,
            preview_lookup_key(&source_hash, &model, &config_hash),
            artifact,
            artifact_set,
            &work_keys,
        )
        .await?,
    ))
}

async fn download_status(
    State(state): State<AppState>,
    Path((slug, format_slug, config_hash)): Path<(String, String, String)>,
) -> Result<Json<ArtifactStatusResponse>, AppError> {
    let model = state
        .db
        .published_catalog_model(&slug)
        .await?
        .ok_or(AppError::NotFound)?;
    let format = catalog::DownloadFormat::from_slug(&format_slug).ok_or(AppError::NotFound)?;
    if !model.exports.downloads.contains(&format) {
        return Err(AppError::NotFound);
    }
    let source_hash = resolve_source_hash(&state, &model).await?;
    let request_hash =
        current_download_request_hash(&state, &model, &source_hash, &config_hash, format).await?;
    let work_keys = request_hash
        .as_deref()
        .map(|request_hash| vec![export_job_key(request_hash)])
        .unwrap_or_default();
    let artifact = if let Some(request_hash) = request_hash.as_deref() {
        ready_download_artifact(&state, request_hash).await?
    } else {
        None
    };
    let artifact_set = if let Some(request_hash) = request_hash.as_deref() {
        latest_artifact_set(&state, request_hash).await?
    } else {
        None
    };

    Ok(Json(
        artifact_status(
            &state,
            download_lookup_key(&source_hash, &model, &config_hash, format),
            artifact,
            artifact_set,
            &work_keys,
        )
        .await?,
    ))
}

async fn artifact_status(
    state: &AppState,
    lookup_key: String,
    artifact: Option<db::ArtifactRecord>,
    artifact_set: Option<db::ArtifactSetRecord>,
    work_keys: &[String],
) -> Result<ArtifactStatusResponse, AppError> {
    if let Some(record) = artifact {
        return Ok(ArtifactStatusResponse {
            artifact_key: record.artifact_key.clone(),
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

    let mut selected_job = None;
    for work_key in work_keys {
        if let Some(job) = state.db.job(work_key).await?
            && selected_job
                .as_ref()
                .map(|current: &db::JobRecord| {
                    job_status_priority(&job.status) > job_status_priority(&current.status)
                })
                .unwrap_or(true)
        {
            selected_job = Some(job);
        }
    }

    if let Some(job) = selected_job.as_ref()
        && matches!(job.status.as_str(), "queued" | "running")
    {
        let message = match job.status.as_str() {
            "queued" => "Generation is queued.",
            "running" => "Generation is running.",
            _ => unreachable!("matched queued or running above"),
        };
        return Ok(ArtifactStatusResponse {
            artifact_key: lookup_key,
            status: job.status.clone(),
            message: message.to_owned(),
            public_url: None,
            object_key: None,
            content_type: None,
            size_bytes: None,
            sha256: None,
            job_id: Some(job.work_key.clone()),
            source_hash: None,
            config_hash: None,
            options_hash: None,
            schema_version: None,
            attempt: Some(job.attempt),
            max_attempts: Some(job.max_attempts),
            next_retry_at: job.next_retry_at.clone(),
            error_summary: job.error_summary.clone(),
            updated_at: Some(job.updated_at.clone()),
        });
    }

    if let Some(artifact_set) = artifact_set
        && artifact_set.status != "ready"
    {
        let message = match artifact_set.status.as_str() {
            "staged" => "Artifact upload is staged but not ready yet.",
            "upload_failed" => "Artifact upload verification failed.",
            "repair_required" => "Artifact verification requires repair.",
            "superseded" => "Artifact was superseded and needs to be queued again.",
            _ => "Artifact status is unknown.",
        };
        return Ok(ArtifactStatusResponse {
            artifact_key: artifact_set.artifact_set_hash,
            status: artifact_set.status,
            message: message.to_owned(),
            public_url: None,
            object_key: artifact_set.primary_object_key,
            content_type: None,
            size_bytes: None,
            sha256: None,
            job_id: selected_job.as_ref().map(|job| job.work_key.clone()),
            source_hash: Some(artifact_set.source_hash),
            config_hash: Some(artifact_set.config_hash),
            options_hash: Some(artifact_set.options_hash),
            schema_version: None,
            attempt: selected_job.as_ref().map(|job| job.attempt),
            max_attempts: selected_job.as_ref().map(|job| job.max_attempts),
            next_retry_at: selected_job
                .as_ref()
                .and_then(|job| job.next_retry_at.clone()),
            error_summary: selected_job
                .as_ref()
                .and_then(|job| job.error_summary.clone()),
            updated_at: Some(artifact_set.updated_at),
        });
    }

    if let Some(job) = selected_job {
        let message = match job.status.as_str() {
            "failed" => "Generation failed.",
            "ready" => "Generation completed; artifact is not visible yet.",
            "superseded" => "Generation was superseded and needs to be queued again.",
            _ => "Generation status is unknown.",
        };
        return Ok(ArtifactStatusResponse {
            artifact_key: lookup_key,
            status: job.status,
            message: message.to_owned(),
            public_url: None,
            object_key: None,
            content_type: None,
            size_bytes: None,
            sha256: None,
            job_id: Some(job.work_key),
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
        artifact_key: lookup_key,
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
    let catalog = state.db.catalog().await?;
    for model in catalog.models() {
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
        let preview_prepared =
            prepare_preview_export(state, model, &source_hash, &validated, &config_hash).await?;
        if ready_preview_artifact(state, &preview_prepared.request_hash)
            .await?
            .is_none()
            && enqueue_preview(state, model, &validated).await?
        {
            enqueued += 1;
        }
        for format in &model.exports.downloads {
            let download_prepared = prepare_download_export(
                state,
                model,
                &source_hash,
                &validated,
                &config_hash,
                *format,
            )
            .await?;
            if ready_download_artifact(state, &download_prepared.request_hash)
                .await?
                .is_none()
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

    if should_retire_legacy_export_job(&job) {
        if !state
            .db
            .finish_job(
                &job.work_key,
                job.attempt,
                "superseded",
                Some("legacy export jobs are retired after the cache v2 hard cut"),
            )
            .await?
        {
            tracing::warn!(work_key = %job.work_key, attempt = job.attempt, "legacy export job lease was already reclaimed before retirement");
        }
        tracing::info!(work_key = %job.work_key, job_kind = %job.job_kind, "retired legacy export job outside the request-hash execution path");
        return Ok(true);
    }

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

fn should_retire_legacy_export_job(job: &db::JobLease) -> bool {
    matches!(job.job_kind.as_str(), "preview_export" | "download_export")
        && !job.work_key.starts_with(V2_EXPORT_WORK_KEY_PREFIX)
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
    let catalog = state.db.catalog().await?;
    match payload {
        JobPayload::ParameterRefresh { model_slug } => {
            let model = catalog
                .find(&model_slug)
                .ok_or_else(|| anyhow::anyhow!("unknown model slug: {model_slug}"))?;
            refresh_parameters(state, model).await?;
        }
        JobPayload::PreviewGlb {
            model_slug,
            request_hash,
            source_hash: queued_source_hash,
            config_hash,
            options_hash,
            output_kind,
            format,
            config_hash_version,
            values,
        } => {
            anyhow::ensure!(
                job.job_kind == "preview_export",
                "unexpected preview job kind"
            );
            let model = catalog
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
            if !queued_source_hash.is_empty() {
                anyhow::ensure!(
                    queued_source_hash == source_hash,
                    "queued preview source hash no longer matches resolved source"
                );
            }
            if !options_hash.is_empty() {
                anyhow::ensure!(
                    options_hash == preview_options_hash(model),
                    "queued preview options hash no longer matches current export options"
                );
            }
            if !output_kind.is_empty() {
                anyhow::ensure!(
                    output_kind == "preview",
                    "unexpected queued preview output kind"
                );
            }
            if !format.is_empty() {
                anyhow::ensure!(format == "glb", "unexpected queued preview export format");
            }
            persist_configuration_selection(state, &source_hash, &validated).await?;
            if ready_preview_artifact(state, &request_hash)
                .await?
                .is_none()
            {
                refresh_preview(
                    state,
                    model,
                    &source_hash,
                    &validated,
                    &config_hash,
                    None,
                    Some(&job.work_key),
                    (!request_hash.is_empty()).then_some(request_hash.as_str()),
                )
                .await?;
            }
        }
        JobPayload::DownloadExport {
            model_slug,
            request_hash,
            source_hash: queued_source_hash,
            config_hash,
            options_hash,
            output_kind,
            export_format: queued_format,
            config_hash_version,
            values,
            format,
        } => {
            anyhow::ensure!(
                job.job_kind == "download_export",
                "unexpected download job kind"
            );
            let model = catalog
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
            if !queued_source_hash.is_empty() {
                anyhow::ensure!(
                    queued_source_hash == source_hash,
                    "queued download source hash no longer matches resolved source"
                );
            }
            if !options_hash.is_empty() {
                anyhow::ensure!(
                    options_hash == download_options_hash(model, format),
                    "queued download options hash no longer matches current export options"
                );
            }
            if !output_kind.is_empty() {
                anyhow::ensure!(
                    output_kind == "download",
                    "unexpected queued download output kind"
                );
            }
            if !queued_format.is_empty() {
                anyhow::ensure!(
                    queued_format == format.slug(),
                    "queued download format no longer matches current export format"
                );
            }
            persist_configuration_selection(state, &source_hash, &validated).await?;
            if ready_download_artifact(state, &request_hash)
                .await?
                .is_none()
            {
                refresh_download(
                    state,
                    model,
                    &source_hash,
                    &validated,
                    &config_hash,
                    format,
                    None,
                    Some(&job.work_key),
                    (!request_hash.is_empty()).then_some(request_hash.as_str()),
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
    format!("onshape/source/v2/{source_hash}/configuration.raw.json")
}

fn parameter_normalized_key(source_hash: &str, schema_hash: &str) -> String {
    format!("onshape/source/v2/{source_hash}/parameters.normalized/{schema_hash}.json")
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
    render_preview_for_hash(
        state,
        model,
        &source_hash,
        &config_hash,
        "default parameters",
    )
    .await
}

async fn render_preview_for_values(
    state: &AppState,
    model: &catalog::Model,
    validated: &ValidatedConfiguration,
) -> Result<String, AppError> {
    let source_hash = resolve_source_hash(state, model).await?;
    let config_hash = persist_configuration_selection(state, &source_hash, validated).await?;
    render_preview_for_hash(state, model, &source_hash, &config_hash, "these parameters").await
}

async fn render_preview_for_hash(
    state: &AppState,
    model: &catalog::Model,
    source_hash: &str,
    config_hash: &str,
    label: &str,
) -> Result<String, AppError> {
    if let Some(request_hash) =
        current_preview_request_hash(state, model, source_hash, config_hash).await?
        && let Some(record) = ready_preview_artifact(state, &request_hash).await?
    {
        Ok(render_preview_viewer(state, &record.object_key))
    } else {
        Ok(format!("<p>No cached preview for {label} yet.</p>"))
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
        if let Some(request_hash) =
            current_download_request_hash(state, model, &source_hash, &config_hash, *format).await?
            && let Some(record) = ready_download_artifact(state, &request_hash).await?
        {
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

async fn execute_staged_export_request(
    state: &AppState,
    request_hash: &str,
    request: &onshape::CanonicalExportRequest,
) -> anyhow::Result<PersistedRawPayload> {
    if let Some(translation) = state
        .db
        .latest_completed_translation_for_request(request_hash)
        .await?
    {
        if let Some(raw_payload) =
            persisted_raw_payload_for_translation_result(state, request_hash, &translation).await?
        {
            return Ok(raw_payload);
        }
        return download_translation_result(state, request_hash, request, &translation).await;
    }

    if let Some(translation) = state
        .db
        .latest_translation_for_request(request_hash)
        .await?
    {
        if translation.final_response_json.is_some() {
            return start_and_download_translation(state, request_hash, request).await;
        }

        let polled = state
            .onshape
            .poll_translation(request.document_id()?, &translation.translation_id)
            .await?;
        let response_hash = translation_response_hash(
            &translation.translation_id,
            translation.start_response_json.as_deref().unwrap_or("{}"),
            &polled.final_response_json,
            &polled.poll_state_json,
        )?;
        state
            .db
            .update_translation_final(TranslationFinalUpdate {
                translation_id: &translation.translation_id,
                state: &polled.state,
                final_response_json: &polled.final_response_json,
                poll_state_json: &polled.poll_state_json,
                result_external_data_ids_json: &serde_json::to_string(
                    &polled.result_external_data_ids,
                )?,
                result_element_ids_json: &serde_json::to_string(&polled.result_element_ids)?,
                response_hash: Some(&response_hash),
                failure_reason: polled.failure_reason.as_deref(),
            })
            .await?;
        anyhow::ensure!(
            polled.state == "DONE",
            "Onshape translation {} failed: {}",
            translation.translation_id,
            polled
                .failure_reason
                .as_deref()
                .unwrap_or(&polled.final_response_json)
        );
        let external_data_id = polled.single_external_data_id()?.to_owned();
        if let Some(raw_payload) = existing_raw_payload_for_source(
            state,
            request_hash,
            &translation.translation_id,
            &external_data_id,
        )
        .await?
        {
            return Ok(raw_payload);
        }
        let downloaded = state
            .onshape
            .download_external_data(request.document_id()?, &external_data_id)
            .await?;
        return persist_downloaded_raw_payload(
            state,
            request_hash,
            &translation.translation_id,
            &external_data_id,
            downloaded,
        )
        .await;
    }

    start_and_download_translation(state, request_hash, request).await
}

async fn start_and_download_translation(
    state: &AppState,
    request_hash: &str,
    request: &onshape::CanonicalExportRequest,
) -> anyhow::Result<PersistedRawPayload> {
    let started = state.onshape.start_export_request(request).await?;
    state
        .db
        .insert_translation_start(TranslationStartInsert {
            translation_id: &started.translation_id,
            request_hash,
            state: &started.state,
            start_response_json: &started.response_json,
        })
        .await?;

    let polled = state
        .onshape
        .poll_translation(request.document_id()?, &started.translation_id)
        .await?;
    let response_hash = translation_response_hash(
        &started.translation_id,
        &started.response_json,
        &polled.final_response_json,
        &polled.poll_state_json,
    )?;
    state
        .db
        .update_translation_final(TranslationFinalUpdate {
            translation_id: &started.translation_id,
            state: &polled.state,
            final_response_json: &polled.final_response_json,
            poll_state_json: &polled.poll_state_json,
            result_external_data_ids_json: &serde_json::to_string(
                &polled.result_external_data_ids,
            )?,
            result_element_ids_json: &serde_json::to_string(&polled.result_element_ids)?,
            response_hash: Some(&response_hash),
            failure_reason: polled.failure_reason.as_deref(),
        })
        .await?;
    anyhow::ensure!(
        polled.state == "DONE",
        "Onshape translation {} failed: {}",
        started.translation_id,
        polled
            .failure_reason
            .as_deref()
            .unwrap_or(&polled.final_response_json)
    );
    let external_data_id = polled.single_external_data_id()?.to_owned();
    let downloaded = state
        .onshape
        .download_external_data(request.document_id()?, &external_data_id)
        .await?;
    persist_downloaded_raw_payload(
        state,
        request_hash,
        &started.translation_id,
        &external_data_id,
        downloaded,
    )
    .await
}

async fn download_translation_result(
    state: &AppState,
    request_hash: &str,
    request: &onshape::CanonicalExportRequest,
    translation: &db::TranslationRecord,
) -> anyhow::Result<PersistedRawPayload> {
    anyhow::ensure!(
        translation.state == "DONE",
        "persisted Onshape translation {} is not resumable: {}{}",
        translation.translation_id,
        translation.state,
        translation
            .failure_reason
            .as_deref()
            .map(|reason| format!(": {reason}"))
            .unwrap_or_default()
    );
    let external_data_id = persisted_result_external_data_id(translation)?;
    if let Some(raw_payload) = existing_raw_payload_for_source(
        state,
        request_hash,
        &translation.translation_id,
        &external_data_id,
    )
    .await?
    {
        return Ok(raw_payload);
    }
    let downloaded = state
        .onshape
        .download_external_data(request.document_id()?, &external_data_id)
        .await?;
    persist_downloaded_raw_payload(
        state,
        request_hash,
        &translation.translation_id,
        &external_data_id,
        downloaded,
    )
    .await
}

async fn persisted_raw_payload_for_translation_result(
    state: &AppState,
    request_hash: &str,
    translation: &db::TranslationRecord,
) -> anyhow::Result<Option<PersistedRawPayload>> {
    let external_data_id = persisted_result_external_data_id(translation)?;
    existing_raw_payload_for_source(
        state,
        request_hash,
        &translation.translation_id,
        &external_data_id,
    )
    .await
}

async fn existing_raw_payload_for_source(
    state: &AppState,
    request_hash: &str,
    translation_id: &str,
    external_data_id: &str,
) -> anyhow::Result<Option<PersistedRawPayload>> {
    let Some(raw_payload_hash) = state
        .db
        .raw_payload_hash_for_source(
            request_hash,
            Some(translation_id),
            Some(external_data_id),
            Some(0),
        )
        .await?
    else {
        return Ok(None);
    };

    let exists = state.db.raw_payload(&raw_payload_hash).await?.is_some();
    anyhow::ensure!(
        exists,
        "raw payload source mapping points to missing payload record {raw_payload_hash}"
    );
    Ok(Some(PersistedRawPayload { raw_payload_hash }))
}

fn persisted_result_external_data_id(
    translation: &db::TranslationRecord,
) -> anyhow::Result<String> {
    let external_data_ids = parse_json_string_vec(
        translation
            .result_external_data_ids_json
            .as_deref()
            .unwrap_or("[]"),
    )?;
    match external_data_ids.as_slice() {
        [external_data_id] => Ok(external_data_id.clone()),
        [] => anyhow::bail!(
            "persisted Onshape translation {} completed without external data",
            translation.translation_id
        ),
        _ => anyhow::bail!(
            "persisted Onshape translation {} returned {} downloadable results; expected exactly one",
            translation.translation_id,
            external_data_ids.len()
        ),
    }
}

fn parse_json_string_vec(json: &str) -> anyhow::Result<Vec<String>> {
    Ok(serde_json::from_str(json)?)
}

fn translation_response_hash(
    translation_id: &str,
    start_response_json: &str,
    final_response_json: &str,
    poll_state_json: &str,
) -> anyhow::Result<String> {
    cache_model::response_hash(&cache_model::ResponseIdentity {
        translation_id: translation_id.to_owned(),
        start_response: serde_json::from_str::<Value>(start_response_json)?,
        final_response: serde_json::from_str::<Value>(final_response_json)?,
        poll_state: serde_json::from_str::<Value>(poll_state_json)?,
        response_shape_version: cache_model::RESPONSE_SHAPE_VERSION,
    })
}

async fn persist_downloaded_raw_payload(
    state: &AppState,
    request_hash: &str,
    translation_id: &str,
    external_data_id: &str,
    downloaded: onshape::DownloadedExternalData,
) -> anyhow::Result<PersistedRawPayload> {
    let raw_payload_hash = cache_key::hex_sha256(&downloaded.bytes);
    let object_key = raw_payload_object_key(&raw_payload_hash);
    let detected_kind = detect_raw_payload_kind(&downloaded.bytes);
    let zip_manifest_json =
        zip_inventory_json(&downloaded.bytes).context("inspecting raw payload ZIP inventory")?;
    state
        .storage
        .put_bytes(
            &object_key,
            downloaded.bytes.clone(),
            downloaded
                .content_type
                .as_deref()
                .unwrap_or("application/octet-stream"),
        )
        .await?;
    state
        .db
        .insert_raw_payload_if_absent(RawPayloadInsert {
            raw_payload_hash: &raw_payload_hash,
            object_key: &object_key,
            content_type: downloaded.content_type.as_deref(),
            byte_len: downloaded.bytes.len() as i64,
            headers_json: &downloaded.response_headers_json,
            original_filename: downloaded.original_filename.as_deref(),
            filename_source: downloaded.filename_source.as_deref(),
            detected_kind,
            zip_manifest_json: zip_manifest_json.as_deref(),
        })
        .await?;
    let linked = state
        .db
        .link_raw_payload_source(RawPayloadSourceInsert {
            request_hash,
            translation_id: Some(translation_id),
            external_data_id: Some(external_data_id),
            result_index: Some(0),
            response_headers_json: &downloaded.response_headers_json,
            etag: downloaded.etag.as_deref(),
            raw_payload_hash: &raw_payload_hash,
        })
        .await?;
    if !linked {
        let existing = state
            .db
            .raw_payload_hash_for_source(
                request_hash,
                Some(translation_id),
                Some(external_data_id),
                Some(0),
            )
            .await?;
        anyhow::ensure!(
            existing.as_deref() == Some(raw_payload_hash.as_str()),
            "raw payload source mapping already exists with a different payload hash"
        );
    }
    Ok(PersistedRawPayload { raw_payload_hash })
}

async fn load_persisted_raw_payload(
    state: &AppState,
    raw_payload_hash: &str,
) -> anyhow::Result<(db::RawPayloadRecord, Vec<u8>)> {
    let record = state
        .db
        .raw_payload(raw_payload_hash)
        .await?
        .with_context(|| format!("missing raw payload record for {raw_payload_hash}"))?;
    let bytes = state.storage.get_bytes(&record.object_key).await?;
    verify_raw_payload_bytes(raw_payload_hash, &bytes)?;
    Ok((record, bytes))
}

fn verify_raw_payload_bytes(raw_payload_hash: &str, bytes: &[u8]) -> anyhow::Result<()> {
    let actual_hash = cache_key::hex_sha256(bytes);
    anyhow::ensure!(
        actual_hash == raw_payload_hash,
        "raw payload bytes sha256 mismatch: expected {raw_payload_hash}, got {actual_hash}"
    );
    Ok(())
}

fn raw_payload_object_key(raw_payload_hash: &str) -> String {
    let prefix_len = raw_payload_hash.len().min(2);
    format!(
        "onshape/raw/v2/{}/{}/payload.bin",
        &raw_payload_hash[..prefix_len],
        raw_payload_hash
    )
}

fn detect_raw_payload_kind(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(b"PK\x03\x04") {
        "zip"
    } else if bytes.starts_with(b"glTF") {
        "glb"
    } else if validate_gltf_json(bytes).is_ok() {
        "gltf_json"
    } else {
        "binary"
    }
}

fn zip_inventory_json(bytes: &[u8]) -> anyhow::Result<Option<String>> {
    if !bytes.starts_with(b"PK\x03\x04") {
        return Ok(None);
    }

    let mut archive = zip::ZipArchive::new(Cursor::new(bytes.to_vec()))?;
    let mut entries = Vec::new();
    for index in 0..archive.len() {
        let file = archive.by_index(index)?;
        if file.is_dir() {
            continue;
        }
        entries.push(RawZipEntry {
            path: safe_zip_asset_name(file.name())?,
            byte_len: file.size(),
        });
    }

    Ok(Some(serde_json::to_string(&entries)?))
}

async fn verify_uploaded_object(
    state: &AppState,
    object_key: &str,
    expected_content_type: &str,
    expected_len: i64,
) -> anyhow::Result<()> {
    let metadata = state.storage.head_object(object_key).await?;
    verify_uploaded_object_metadata(&metadata, object_key, expected_content_type, expected_len)
}

fn verify_uploaded_object_metadata(
    metadata: &storage::ObjectMetadata,
    object_key: &str,
    expected_content_type: &str,
    expected_len: i64,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        metadata.content_length == expected_len,
        "uploaded object length mismatch for {object_key}: expected {expected_len}, got {}",
        metadata.content_length
    );
    if let Some(content_type) = metadata.content_type.as_deref() {
        anyhow::ensure!(
            content_type == expected_content_type,
            "uploaded object content type mismatch for {object_key}: expected {expected_content_type}, got {content_type}"
        );
    }
    Ok(())
}

async fn verify_uploaded_object_read_back(
    state: &AppState,
    object_key: &str,
    expected_len: i64,
    expected_sha256: &str,
) -> anyhow::Result<()> {
    let bytes = state.storage.get_bytes(object_key).await?;
    verify_uploaded_object_bytes(&bytes, object_key, expected_len, expected_sha256)
}

fn verify_uploaded_object_bytes(
    bytes: &[u8],
    object_key: &str,
    expected_len: i64,
    expected_sha256: &str,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        bytes.len() as i64 == expected_len,
        "uploaded object read-back length mismatch for {object_key}: expected {expected_len}, got {}",
        bytes.len()
    );
    let actual_sha256 = cache_key::hex_sha256(bytes);
    anyhow::ensure!(
        actual_sha256 == expected_sha256,
        "uploaded object read-back sha256 mismatch for {object_key}: expected {expected_sha256}, got {actual_sha256}"
    );
    Ok(())
}

async fn verify_uploaded_artifact(
    state: &AppState,
    object_key: &str,
    expected_content_type: &str,
    expected_len: i64,
    expected_sha256: &str,
) -> anyhow::Result<()> {
    verify_uploaded_object(state, object_key, expected_content_type, expected_len).await?;
    if STRICT_UPLOAD_VERIFICATION {
        verify_uploaded_object_read_back(state, object_key, expected_len, expected_sha256).await?;
    }
    Ok(())
}

async fn mark_artifact_upload_failed(
    state: &AppState,
    artifact_key: &str,
    error: &anyhow::Error,
) -> anyhow::Result<()> {
    state
        .db
        .transition_artifact_set_status(artifact_key, "upload_failed")
        .await
        .with_context(|| format!("marking artifact set upload_failed for {artifact_key}"))?;
    tracing::warn!(artifact_key = %artifact_key, error = %error, "artifact upload verification failed");
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn refresh_preview(
    state: &AppState,
    model: &catalog::Model,
    source_hash: &str,
    validated: &ValidatedConfiguration,
    config_hash: &str,
    prepared: Option<PreparedExportRequest>,
    producing_job_key: Option<&str>,
    expected_request_hash: Option<&str>,
) -> anyhow::Result<String> {
    let prepared = match prepared {
        Some(prepared) => prepared,
        None => prepare_preview_export(state, model, source_hash, validated, config_hash).await?,
    };
    if let Some(expected_request_hash) = expected_request_hash {
        anyhow::ensure!(
            prepared.request_hash == expected_request_hash,
            "queued preview request hash no longer matches the canonical export request"
        );
    }
    let raw_payload =
        execute_staged_export_request(state, &prepared.request_hash, &prepared.request).await?;
    let config_values_json = config_values_json(&validated.values)?;
    let preview_artifact =
        match postprocess_preview_artifact(state, &raw_payload.raw_payload_hash).await {
            Ok(preview_artifact) => preview_artifact,
            Err(error) => {
                tracing::warn!(
                    request_hash = %prepared.request_hash,
                    raw_payload_hash = %raw_payload.raw_payload_hash,
                    error = %error,
                    "preview post-processing failed"
                );
                return Err(error);
            }
        };
    let artifact_key = preview_artifact_key(
        source_hash,
        config_hash,
        &prepared.options_hash,
        if preview_artifact.content_type == "model/gltf-binary" {
            "glb"
        } else {
            "gltf"
        },
        &prepared.request_hash,
        &raw_payload.raw_payload_hash,
        &preview_artifact.postprocess_hash,
    )?;
    let primary_object_key =
        preview_asset_object_key(&artifact_key, &preview_artifact.logical_path);
    let sidecar_object_keys = preview_artifact
        .sidecars
        .iter()
        .map(|sidecar| preview_asset_object_key(&artifact_key, &sidecar.logical_path))
        .collect::<Vec<_>>();
    let preview_sha256 = cache_key::hex_sha256(&preview_artifact.bytes);
    let sidecar_sha256 = preview_artifact
        .sidecars
        .iter()
        .map(|sidecar| cache_key::hex_sha256(&sidecar.bytes))
        .collect::<Vec<_>>();
    let mut artifact_files = vec![db::ArtifactFileInsert {
        artifact_set_hash: &artifact_key,
        role: "viewer_entry",
        logical_path: &preview_artifact.logical_path,
        original_path: preview_artifact.original_path.as_deref(),
        object_key: &primary_object_key,
        content_type: preview_artifact.content_type,
        byte_len: preview_artifact.bytes.len() as i64,
        sha256: &preview_sha256,
        metadata_json: "{}",
    }];
    artifact_files.extend(
        preview_artifact
            .sidecars
            .iter()
            .zip(sidecar_object_keys.iter())
            .zip(sidecar_sha256.iter())
            .map(|((sidecar, object_key), sha256)| db::ArtifactFileInsert {
                artifact_set_hash: &artifact_key,
                role: sidecar.role,
                logical_path: &sidecar.logical_path,
                original_path: sidecar.original_path.as_deref(),
                object_key,
                content_type: sidecar.content_type,
                byte_len: sidecar.bytes.len() as i64,
                sha256,
                metadata_json: "{}",
            }),
    );
    state
        .db
        .stage_artifact(
            ArtifactUpsert {
                artifact_key: &artifact_key,
                model_slug: &model.slug,
                config_hash,
                output_kind: "preview_glb",
                format: if preview_artifact.content_type == "model/gltf-binary" {
                    "glb"
                } else {
                    "gltf"
                },
                object_key: &primary_object_key,
                content_type: preview_artifact.content_type,
                byte_len: preview_artifact.bytes.len() as i64,
                sha256: &preview_sha256,
                producing_job_key,
                source_hash,
                options_hash: &prepared.options_hash,
                request_hash: Some(&prepared.request_hash),
                raw_payload_hash: Some(&raw_payload.raw_payload_hash),
                postprocess_hash: Some(&preview_artifact.postprocess_hash),
                parameter_schema_version: SCHEMA_VERSION.into(),
                config_values_json: &config_values_json,
            },
            &artifact_files,
        )
        .await?;
    let upload_result: anyhow::Result<()> = async {
        state
            .storage
            .put_bytes(
                &primary_object_key,
                preview_artifact.bytes.clone(),
                preview_artifact.content_type,
            )
            .await?;
        verify_uploaded_artifact(
            state,
            &primary_object_key,
            preview_artifact.content_type,
            preview_artifact.bytes.len() as i64,
            &preview_sha256,
        )
        .await?;
        for ((sidecar, object_key), sidecar_sha256) in preview_artifact
            .sidecars
            .iter()
            .zip(sidecar_object_keys.iter())
            .zip(sidecar_sha256.iter())
        {
            state
                .storage
                .put_bytes(object_key, sidecar.bytes.clone(), sidecar.content_type)
                .await?;
            verify_uploaded_artifact(
                state,
                object_key,
                sidecar.content_type,
                sidecar.bytes.len() as i64,
                sidecar_sha256,
            )
            .await?;
        }
        Ok(())
    }
    .await;
    if let Err(error) = upload_result {
        mark_artifact_upload_failed(state, &artifact_key, &error).await?;
        return Err(error);
    }
    state
        .db
        .mark_artifact_set_ready(&artifact_key, &primary_object_key)
        .await?;
    state
        .db
        .supersede_ready_artifacts_for_output(
            source_hash,
            config_hash,
            &prepared.options_hash,
            "preview_glb",
            &artifact_key,
            Some("replaced"),
        )
        .await?;
    tracing::debug!(request_hash = %prepared.request_hash, raw_payload_hash = %raw_payload.raw_payload_hash, %artifact_key, "staged preview export request");
    Ok(primary_object_key)
}

fn preview_artifact_from_onshape_bytes(bytes: Vec<u8>) -> anyhow::Result<PreviewArtifact> {
    if bytes.starts_with(b"glTF") {
        validate_glb(&bytes).context("validating direct GLB preview export")?;
        return Ok(PreviewArtifact {
            logical_path: "preview.glb".to_owned(),
            original_path: None,
            content_type: "model/gltf-binary",
            bytes,
            sidecars: Vec::new(),
        });
    }

    if bytes.starts_with(b"PK\x03\x04") {
        return preview_artifact_from_zip(bytes);
    }

    validate_gltf_json(&bytes).context("validating direct glTF preview export")?;
    Ok(PreviewArtifact {
        logical_path: "preview.gltf".to_owned(),
        original_path: None,
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

fn preview_artifact_from_zip(bytes: Vec<u8>) -> anyhow::Result<PreviewArtifact> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))?;
    let glb_entries = zip_entry_indices_with_extension(&mut archive, "glb")?;
    match glb_entries.as_slice() {
        [index] => {
            let bytes = read_zip_entry(&mut archive, *index)?;
            validate_glb(&bytes).context("validating zipped GLB preview export")?;
            Ok(PreviewArtifact {
                logical_path: "preview.glb".to_owned(),
                original_path: Some(safe_zip_asset_name(archive.by_index(*index)?.name())?),
                content_type: "model/gltf-binary",
                bytes,
                sidecars: Vec::new(),
            })
        }
        [] => preview_artifact_from_gltf_zip(archive),
        _ => anyhow::bail!(
            "Onshape preview ZIP contained multiple GLB files; expected exactly one GLB preview artifact"
        ),
    }
}

fn preview_artifact_from_gltf_zip(
    mut archive: zip::ZipArchive<Cursor<Vec<u8>>>,
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
    let referenced_assets = referenced_gltf_sidecar_assets(&primary_name, &primary_bytes)
        .context("validating zipped glTF preview JSON")?;

    let mut sidecars = Vec::new();
    let mut sidecar_paths = HashSet::new();
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
        sidecar_paths.insert(asset_name.clone());
        sidecars.push(PreviewAsset {
            role: "sidecar",
            logical_path: asset_name.clone(),
            original_path: Some(asset_name.clone()),
            content_type: preview_asset_content_type(&asset_name),
            bytes,
        });
    }

    for referenced_asset in referenced_assets {
        anyhow::ensure!(
            sidecar_paths.contains(&referenced_asset),
            "glTF preview ZIP is missing referenced sidecar asset: {referenced_asset}"
        );
    }

    Ok(PreviewArtifact {
        logical_path: primary_name.clone(),
        original_path: Some(primary_name.clone()),
        content_type: "model/gltf+json",
        bytes: primary_bytes,
        sidecars,
    })
}

fn referenced_gltf_sidecar_assets(
    primary_name: &str,
    bytes: &[u8],
) -> anyhow::Result<HashSet<String>> {
    let value = serde_json::from_slice::<Value>(bytes)?;
    anyhow::ensure!(
        value
            .get("asset")
            .and_then(|asset| asset.get("version"))
            .and_then(Value::as_str)
            .is_some(),
        "glTF JSON did not include asset.version"
    );

    let mut assets = HashSet::new();
    collect_gltf_uri_references(primary_name, value.get("buffers"), "buffer", &mut assets)?;
    collect_gltf_uri_references(primary_name, value.get("images"), "image", &mut assets)?;
    Ok(assets)
}

fn collect_gltf_uri_references(
    primary_name: &str,
    values: Option<&Value>,
    kind: &str,
    assets: &mut HashSet<String>,
) -> anyhow::Result<()> {
    let Some(values) = values.and_then(Value::as_array) else {
        return Ok(());
    };
    for value in values {
        let Some(uri) = value.get("uri").and_then(Value::as_str) else {
            continue;
        };
        let normalized = normalize_gltf_reference_uri(primary_name, uri)
            .with_context(|| format!("glTF {kind} URI is unsupported: {uri}"))?;
        assets.insert(normalized);
    }
    Ok(())
}

fn normalize_gltf_reference_uri(primary_name: &str, uri: &str) -> anyhow::Result<String> {
    anyhow::ensure!(!uri.is_empty(), "URI is empty");
    anyhow::ensure!(
        uri == uri.trim(),
        "URI must not have surrounding whitespace"
    );
    let lower = uri.to_ascii_lowercase();
    anyhow::ensure!(
        !lower.starts_with("data:"),
        "data URI sidecars are not supported"
    );
    anyhow::ensure!(
        !uri.contains("://"),
        "external URI sidecars are not supported"
    );
    anyhow::ensure!(
        !uri.starts_with('/') && !uri.starts_with('\\'),
        "absolute sidecar paths are not supported"
    );
    anyhow::ensure!(
        !uri.contains('\\'),
        "backslash sidecar paths are not supported"
    );
    anyhow::ensure!(
        !uri.contains('?') && !uri.contains('#'),
        "URI query and fragment components are not supported"
    );

    let mut parts = Vec::new();
    if let Some(parent) = std::path::Path::new(primary_name).parent() {
        for component in parent.components() {
            if let std::path::Component::Normal(part) = component {
                parts.push(part.to_string_lossy().into_owned());
            }
        }
    }
    for part in uri.split('/') {
        anyhow::ensure!(!part.is_empty(), "URI contains an empty path segment");
        anyhow::ensure!(
            part != "." && part != "..",
            "URI contains an unsafe path segment"
        );
        anyhow::ensure!(
            !part.chars().any(char::is_control),
            "URI contains control characters"
        );
        parts.push(part.to_owned());
    }
    Ok(parts.join("/"))
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

#[allow(clippy::too_many_arguments)]
async fn refresh_download(
    state: &AppState,
    model: &catalog::Model,
    source_hash: &str,
    validated: &ValidatedConfiguration,
    config_hash: &str,
    format: catalog::DownloadFormat,
    prepared: Option<PreparedExportRequest>,
    producing_job_key: Option<&str>,
    expected_request_hash: Option<&str>,
) -> anyhow::Result<String> {
    let prepared = match prepared {
        Some(prepared) => prepared,
        None => {
            prepare_download_export(state, model, source_hash, validated, config_hash, format)
                .await?
        }
    };
    if let Some(expected_request_hash) = expected_request_hash {
        anyhow::ensure!(
            prepared.request_hash == expected_request_hash,
            "queued download request hash no longer matches the canonical export request"
        );
    }
    let raw_payload =
        execute_staged_export_request(state, &prepared.request_hash, &prepared.request).await?;
    let config_values_json = config_values_json(&validated.values)?;
    let download_artifact =
        match postprocess_download_artifact(state, model, format, &raw_payload.raw_payload_hash)
            .await
        {
            Ok(download_artifact) => download_artifact,
            Err(error) => {
                tracing::warn!(
                    request_hash = %prepared.request_hash,
                    raw_payload_hash = %raw_payload.raw_payload_hash,
                    error = %error,
                    "download post-processing failed"
                );
                return Err(error);
            }
        };
    let artifact_key = download_artifact_key(
        source_hash,
        config_hash,
        &prepared.options_hash,
        format.slug(),
        &prepared.request_hash,
        &raw_payload.raw_payload_hash,
        &download_artifact.postprocess_hash,
    )?;
    let object_key = download_object_key(&artifact_key, &download_artifact.logical_path);
    let content_disposition = format!(
        "attachment; filename=\"{}\"",
        download_artifact.logical_path
    );
    let download_sha256 = cache_key::hex_sha256(&download_artifact.bytes);
    let artifact_files = [db::ArtifactFileInsert {
        artifact_set_hash: &artifact_key,
        role: "download",
        logical_path: &download_artifact.logical_path,
        original_path: Some(&download_artifact.logical_path),
        object_key: &object_key,
        content_type: format.content_type(),
        byte_len: download_artifact.bytes.len() as i64,
        sha256: &download_sha256,
        metadata_json: "{}",
    }];
    state
        .db
        .stage_artifact(
            ArtifactUpsert {
                artifact_key: &artifact_key,
                model_slug: &model.slug,
                config_hash,
                output_kind: format.slug(),
                format: format.slug(),
                object_key: &object_key,
                content_type: format.content_type(),
                byte_len: download_artifact.bytes.len() as i64,
                sha256: &download_sha256,
                producing_job_key,
                source_hash,
                options_hash: &prepared.options_hash,
                request_hash: Some(&prepared.request_hash),
                raw_payload_hash: Some(&raw_payload.raw_payload_hash),
                postprocess_hash: Some(&download_artifact.postprocess_hash),
                parameter_schema_version: SCHEMA_VERSION.into(),
                config_values_json: &config_values_json,
            },
            &artifact_files,
        )
        .await?;
    let upload_result: anyhow::Result<()> = async {
        state
            .storage
            .put_bytes_with_headers(
                &object_key,
                download_artifact.bytes.clone(),
                format.content_type(),
                Some(&content_disposition),
                Some("public, max-age=31536000, immutable"),
            )
            .await?;
        verify_uploaded_artifact(
            state,
            &object_key,
            format.content_type(),
            download_artifact.bytes.len() as i64,
            &download_sha256,
        )
        .await
    }
    .await;
    if let Err(error) = upload_result {
        mark_artifact_upload_failed(state, &artifact_key, &error).await?;
        return Err(error);
    }
    state
        .db
        .mark_artifact_set_ready(&artifact_key, &object_key)
        .await?;
    state
        .db
        .supersede_ready_artifacts_for_output(
            source_hash,
            config_hash,
            &prepared.options_hash,
            format.slug(),
            &artifact_key,
            Some("replaced"),
        )
        .await?;
    tracing::debug!(request_hash = %prepared.request_hash, raw_payload_hash = %raw_payload.raw_payload_hash, %artifact_key, "staged download export request");
    Ok(object_key)
}

#[derive(Debug)]
struct PostprocessedPreviewArtifact {
    postprocess_hash: String,
    logical_path: String,
    original_path: Option<String>,
    content_type: &'static str,
    bytes: Vec<u8>,
    sidecars: Vec<PreviewAsset>,
}

#[derive(Debug)]
struct PostprocessedDownloadArtifact {
    postprocess_hash: String,
    logical_path: String,
    bytes: Vec<u8>,
}

async fn postprocess_preview_artifact(
    state: &AppState,
    raw_payload_hash: &str,
) -> anyhow::Result<PostprocessedPreviewArtifact> {
    let policy = PreviewPostprocessPolicy {
        accepted_input_shapes: vec![
            "direct_glb",
            "direct_gltf_json",
            "zip_single_glb",
            "zip_single_gltf_asset_set",
        ],
        allow_partial_multi_gltf_preview_fallback: ALLOW_PARTIAL_MULTI_GLTF_PREVIEW_FALLBACK,
    };
    let postprocess_hash = cache_model::postprocess_hash(&cache_model::PostprocessIdentity {
        raw_payload_hash: raw_payload_hash.to_owned(),
        processor_name: PREVIEW_POSTPROCESSOR_NAME.to_owned(),
        processor_version: PREVIEW_POSTPROCESSOR_VERSION.to_owned(),
        policy: &policy,
    })?;
    let policy_json = serde_json::to_string(&policy)?;
    state
        .db
        .insert_postprocess_run_if_absent(db::PostprocessRunInsert {
            postprocess_hash: &postprocess_hash,
            raw_payload_hash,
            processor_name: PREVIEW_POSTPROCESSOR_NAME,
            processor_version: PREVIEW_POSTPROCESSOR_VERSION,
            policy_json: &policy_json,
            status: POSTPROCESS_STATUS_STAGED,
            log_json: "[]",
            derived_files_json: "[]",
        })
        .await?;

    let (_, bytes) = load_persisted_raw_payload(state, raw_payload_hash).await?;
    match preview_artifact_from_onshape_bytes(bytes) {
        Ok(artifact) => {
            let derived_files_json =
                serde_json::to_string(&preview_artifact_derived_files(&artifact))?;
            let log_json = serde_json::to_string(&vec![PostprocessLogEntry {
                level: "info",
                message: "preview post-processing completed".to_owned(),
            }])?;
            state
                .db
                .transition_postprocess_run_status(
                    &postprocess_hash,
                    POSTPROCESS_STATUS_READY,
                    &log_json,
                    &derived_files_json,
                )
                .await?;
            Ok(PostprocessedPreviewArtifact {
                postprocess_hash,
                logical_path: artifact.logical_path,
                original_path: artifact.original_path,
                content_type: artifact.content_type,
                bytes: artifact.bytes,
                sidecars: artifact.sidecars,
            })
        }
        Err(error) => {
            let log_json = serde_json::to_string(&vec![PostprocessLogEntry {
                level: "error",
                message: error.to_string(),
            }])?;
            state
                .db
                .transition_postprocess_run_status(
                    &postprocess_hash,
                    POSTPROCESS_STATUS_FAILED,
                    &log_json,
                    "[]",
                )
                .await?;
            Err(error)
        }
    }
}

async fn postprocess_download_artifact(
    state: &AppState,
    model: &catalog::Model,
    format: catalog::DownloadFormat,
    raw_payload_hash: &str,
) -> anyhow::Result<PostprocessedDownloadArtifact> {
    let policy = DownloadPostprocessPolicy {
        strategy: "identity",
        format: format.slug(),
        content_type: format.content_type(),
    };
    let postprocess_hash = cache_model::postprocess_hash(&cache_model::PostprocessIdentity {
        raw_payload_hash: raw_payload_hash.to_owned(),
        processor_name: DOWNLOAD_POSTPROCESSOR_NAME.to_owned(),
        processor_version: DOWNLOAD_POSTPROCESSOR_VERSION.to_owned(),
        policy: &policy,
    })?;
    let policy_json = serde_json::to_string(&policy)?;
    state
        .db
        .insert_postprocess_run_if_absent(db::PostprocessRunInsert {
            postprocess_hash: &postprocess_hash,
            raw_payload_hash,
            processor_name: DOWNLOAD_POSTPROCESSOR_NAME,
            processor_version: DOWNLOAD_POSTPROCESSOR_VERSION,
            policy_json: &policy_json,
            status: POSTPROCESS_STATUS_STAGED,
            log_json: "[]",
            derived_files_json: "[]",
        })
        .await?;

    let (_, bytes) = load_persisted_raw_payload(state, raw_payload_hash).await?;
    let logical_path = download_filename(model, format);
    let derived_files_json = serde_json::to_string(&vec![DerivedArtifactFile {
        role: "download",
        logical_path: &logical_path,
        original_path: Some(&logical_path),
        object_key: None,
        content_type: format.content_type(),
        byte_len: bytes.len(),
        sha256: cache_key::hex_sha256(&bytes),
    }])?;
    let log_json = serde_json::to_string(&vec![PostprocessLogEntry {
        level: "info",
        message: "download identity post-processing completed".to_owned(),
    }])?;
    state
        .db
        .transition_postprocess_run_status(
            &postprocess_hash,
            POSTPROCESS_STATUS_READY,
            &log_json,
            &derived_files_json,
        )
        .await?;

    Ok(PostprocessedDownloadArtifact {
        postprocess_hash,
        logical_path,
        bytes,
    })
}

fn preview_artifact_derived_files(artifact: &PreviewArtifact) -> Vec<DerivedArtifactFile<'_>> {
    let mut files = vec![DerivedArtifactFile {
        role: "viewer_entry",
        logical_path: &artifact.logical_path,
        original_path: artifact.original_path.as_deref(),
        object_key: None,
        content_type: artifact.content_type,
        byte_len: artifact.bytes.len(),
        sha256: cache_key::hex_sha256(&artifact.bytes),
    }];
    files.extend(artifact.sidecars.iter().map(|sidecar| DerivedArtifactFile {
        role: sidecar.role,
        logical_path: &sidecar.logical_path,
        original_path: sidecar.original_path.as_deref(),
        object_key: None,
        content_type: sidecar.content_type,
        byte_len: sidecar.bytes.len(),
        sha256: cache_key::hex_sha256(&sidecar.bytes),
    }));
    files
}

async fn prepare_preview_export(
    state: &AppState,
    model: &catalog::Model,
    source_hash: &str,
    validated: &ValidatedConfiguration,
    config_hash: &str,
) -> anyhow::Result<PreparedExportRequest> {
    let configuration = resolve_configuration_encoding(
        state,
        model,
        source_hash,
        config_hash,
        &encoding_request_values(&validated.typed_values),
    )
    .await?;
    let options_hash = preview_options_hash(model);
    let request = state.onshape.build_preview_glb_export_request(
        &model.onshape,
        &EncodedConfigurationIdentity {
            encoded_id: configuration.encoded_id,
            query_param: configuration.query_param,
        },
        &model.exports.preview_options,
    );
    let request_hash = persist_export_request(
        state,
        source_hash,
        config_hash,
        &options_hash,
        "preview",
        "glb",
        &request,
    )
    .await?;
    Ok(PreparedExportRequest {
        options_hash,
        request_hash,
        request,
    })
}

async fn prepare_download_export(
    state: &AppState,
    model: &catalog::Model,
    source_hash: &str,
    validated: &ValidatedConfiguration,
    config_hash: &str,
    format: catalog::DownloadFormat,
) -> anyhow::Result<PreparedExportRequest> {
    let configuration = resolve_configuration_encoding(
        state,
        model,
        source_hash,
        config_hash,
        &encoding_request_values(&validated.typed_values),
    )
    .await?;
    let options_hash = download_options_hash(model, format);
    let request = state.onshape.build_download_export_request(
        &model.onshape,
        &EncodedConfigurationIdentity {
            encoded_id: configuration.encoded_id,
            query_param: configuration.query_param,
        },
        format,
        &model.exports.download_options,
    );
    let request_hash = persist_export_request(
        state,
        source_hash,
        config_hash,
        &options_hash,
        "download",
        format.slug(),
        &request,
    )
    .await?;
    Ok(PreparedExportRequest {
        options_hash,
        request_hash,
        request,
    })
}

async fn persist_export_request(
    state: &AppState,
    source_hash: &str,
    config_hash: &str,
    options_hash: &str,
    output_kind: &str,
    format: &str,
    request: &onshape::CanonicalExportRequest,
) -> anyhow::Result<String> {
    let request_hash = cache_model::request_hash(&request.identity)?;
    let request_json = request.request_json()?;
    state
        .db
        .insert_export_request_if_absent(ExportRequestInsert {
            request_hash: &request_hash,
            source_hash,
            config_hash,
            options_hash,
            output_kind,
            format,
            endpoint: &request.operation,
            method: &request.method,
            path: &request.path,
            request_json: &request_json,
            defaults_policy_version: &request.identity.defaults_policy_version,
            request_builder_version: &request.identity.request_builder_version,
            status: EXPORT_REQUEST_STATUS_STAGED,
        })
        .await?;
    Ok(request_hash)
}

async fn supersede_published_artifact(
    state: &AppState,
    artifact: &db::ArtifactRecord,
) -> anyhow::Result<()> {
    state.db.supersede_artifact(&artifact.artifact_key).await?;
    if let Some(work_key) = artifact.producing_job_key.as_deref() {
        state.db.supersede_ready_job(work_key).await?;
    }
    Ok(())
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

fn export_job_key(request_hash: &str) -> String {
    format!("{V2_EXPORT_WORK_KEY_PREFIX}{request_hash}")
}

#[cfg(test)]
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

#[cfg(test)]
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

#[cfg(test)]
async fn export_status_job_keys(
    state: &AppState,
    source_hash: &str,
    config_hash: &str,
    options_hash: &str,
    output_kind: &str,
    format: &str,
) -> Result<Vec<String>, AppError> {
    let mut work_keys = Vec::with_capacity(1);
    if let Some(request) = state
        .db
        .latest_export_request_for_output(
            source_hash,
            config_hash,
            options_hash,
            output_kind,
            format,
        )
        .await?
    {
        work_keys.push(export_job_key(&request.request_hash));
    }
    Ok(work_keys)
}

fn job_status_priority(status: &str) -> u8 {
    match status {
        "running" => 5,
        "queued" => 4,
        "ready" => 3,
        "failed" => 2,
        "superseded" => 1,
        _ => 0,
    }
}

async fn ready_preview_artifact(
    state: &AppState,
    request_hash: &str,
) -> sqlx::Result<Option<db::ArtifactRecord>> {
    state
        .db
        .latest_ready_artifact_for_request(request_hash)
        .await
}

async fn ready_download_artifact(
    state: &AppState,
    request_hash: &str,
) -> sqlx::Result<Option<db::ArtifactRecord>> {
    state
        .db
        .latest_ready_artifact_for_request(request_hash)
        .await
}

async fn latest_artifact_set(
    state: &AppState,
    request_hash: &str,
) -> sqlx::Result<Option<db::ArtifactSetRecord>> {
    state.db.latest_artifact_set_for_request(request_hash).await
}

async fn current_preview_request_hash(
    state: &AppState,
    model: &catalog::Model,
    source_hash: &str,
    config_hash: &str,
) -> anyhow::Result<Option<String>> {
    let Some(configuration) = state
        .db
        .configuration_encoding(source_hash, config_hash)
        .await?
    else {
        return Ok(None);
    };
    let request = state.onshape.build_preview_glb_export_request(
        &model.onshape,
        &EncodedConfigurationIdentity {
            encoded_id: configuration.encoded_id,
            query_param: configuration.query_param,
        },
        &model.exports.preview_options,
    );
    Ok(Some(cache_model::request_hash(&request.identity)?))
}

async fn current_download_request_hash(
    state: &AppState,
    model: &catalog::Model,
    source_hash: &str,
    config_hash: &str,
    format: catalog::DownloadFormat,
) -> anyhow::Result<Option<String>> {
    let Some(configuration) = state
        .db
        .configuration_encoding(source_hash, config_hash)
        .await?
    else {
        return Ok(None);
    };
    let request = state.onshape.build_download_export_request(
        &model.onshape,
        &EncodedConfigurationIdentity {
            encoded_id: configuration.encoded_id,
            query_param: configuration.query_param,
        },
        format,
        &model.exports.download_options,
    );
    Ok(Some(cache_model::request_hash(&request.identity)?))
}

fn preview_lookup_key(source_hash: &str, model: &catalog::Model, config_hash: &str) -> String {
    format!(
        "artifact-lookup-v2:preview_glb:{}:{config_hash}:{}",
        source_hash,
        preview_options_hash(model),
    )
}

fn preview_artifact_key(
    source_hash: &str,
    config_hash: &str,
    options_hash: &str,
    format: &str,
    request_hash: &str,
    raw_payload_hash: &str,
    postprocess_hash: &str,
) -> anyhow::Result<String> {
    cache_model::artifact_set_hash(&cache_model::ArtifactSetIdentity {
        artifact_set_schema_version: cache_model::ARTIFACT_SET_SCHEMA_VERSION,
        output_kind: "preview_glb".to_owned(),
        format: format.to_owned(),
        source_hash: source_hash.to_owned(),
        config_hash: config_hash.to_owned(),
        options_hash: options_hash.to_owned(),
        request_hash: request_hash.to_owned(),
        raw_payload_hash: raw_payload_hash.to_owned(),
        postprocess_hash: postprocess_hash.to_owned(),
    })
}

#[cfg(test)]
fn preview_glb_object_key(artifact_set_hash: &str) -> String {
    preview_asset_object_key(artifact_set_hash, "preview.glb")
}

fn preview_asset_object_key(artifact_set_hash: &str, asset_name: &str) -> String {
    format!("previews/v2/{artifact_set_hash}/{asset_name}")
}

fn download_artifact_key(
    source_hash: &str,
    config_hash: &str,
    options_hash: &str,
    format: &str,
    request_hash: &str,
    raw_payload_hash: &str,
    postprocess_hash: &str,
) -> anyhow::Result<String> {
    cache_model::artifact_set_hash(&cache_model::ArtifactSetIdentity {
        artifact_set_schema_version: cache_model::ARTIFACT_SET_SCHEMA_VERSION,
        output_kind: format.to_owned(),
        format: format.to_owned(),
        source_hash: source_hash.to_owned(),
        config_hash: config_hash.to_owned(),
        options_hash: options_hash.to_owned(),
        request_hash: request_hash.to_owned(),
        raw_payload_hash: raw_payload_hash.to_owned(),
        postprocess_hash: postprocess_hash.to_owned(),
    })
}

fn download_lookup_key(
    source_hash: &str,
    model: &catalog::Model,
    config_hash: &str,
    format: catalog::DownloadFormat,
) -> String {
    format!(
        "artifact-lookup-v2:download:{}:{}:{config_hash}:{}",
        format.slug(),
        source_hash,
        download_options_hash(model, format),
    )
}

fn download_object_key(artifact_set_hash: &str, filename: &str) -> String {
    format!("artifacts/v2/{artifact_set_hash}/{filename}")
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
    let request_values = encoding_request_values(&validated.typed_values);
    Ok(serde_json::to_string(&serde_json::json!({
        "parameterSchemaVersion": SCHEMA_VERSION,
        "submittedValues": cache_model::canonical_values(&validated.values),
        "requestValues": request_values,
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
    request_values: &BTreeMap<String, String>,
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
        .encode_configuration(&model.onshape, request_values)
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
    cache_model::options_hash(format, options_version, options).expect("export options serialize")
}

fn work_key(kind: &'static str, payload: &WorkKeyPayload) -> String {
    format!(
        "work-v2:{kind}:{}",
        cache_key::hash_json("work-v2", payload).expect("work key payload serializes")
    )
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

fn normalize_form_values(
    schema: &ParameterSchema,
    raw: HashMap<String, String>,
) -> HashMap<String, String> {
    let mut normalized = raw
        .iter()
        .filter(|(name, _)| !name.ends_with("__value") && !name.ends_with("__unit"))
        .map(|(name, value)| (name.clone(), value.clone()))
        .collect::<HashMap<_, _>>();

    for parameter in &schema.parameters {
        if parameter.kind != ParameterKind::Number || quantity_unit_options(parameter).is_none() {
            continue;
        }
        let value_name = format!("{}__value", parameter.id);
        let Some(value) = raw.get(&value_name).map(|value| value.trim()) else {
            continue;
        };
        if value.is_empty() {
            normalized.remove(&parameter.id);
            continue;
        }

        let unit_name = format!("{}__unit", parameter.id);
        let unit = raw
            .get(&unit_name)
            .map(|unit| unit.trim())
            .filter(|unit| !unit.is_empty())
            .or_else(|| default_quantity_unit(parameter))
            .unwrap_or("");
        normalized.insert(parameter.id.clone(), format!("{value} {unit}"));
    }

    normalized
}

fn split_quantity_display_value(value: &str, default_unit: &str) -> (String, String) {
    let trimmed = value.trim();
    let start = trimmed
        .char_indices()
        .rev()
        .find(|(_, character)| !character.is_ascii_alphabetic())
        .map(|(index, character)| index + character.len_utf8())
        .unwrap_or(0);
    let unit = trimmed[start..].trim();
    if unit.is_empty() {
        (trimmed.to_owned(), default_unit.to_owned())
    } else {
        (trimmed[..start].trim().to_owned(), unit.to_owned())
    }
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
                ParameterKind::Number if quantity_unit_options(parameter).is_some() => {
                    let default_unit = default_quantity_unit(parameter).unwrap_or_default();
                    let (number_value, selected_unit) =
                        split_quantity_display_value(&display_value, default_unit);
                    let options = quantity_unit_options(parameter)
                        .unwrap_or_default()
                        .iter()
                        .map(|option| {
                            let selected = if option.value == selected_unit {
                                " selected"
                            } else {
                                ""
                            };
                            format!(
                                r#"<option value="{value}"{selected}>{label}</option>"#,
                                value = escape_html(option.value),
                                label = escape_html(option.label),
                            )
                        })
                        .collect::<String>();
                    format!(
                        r#"<span class="quantity-control"><input id="{id}" name="{id}__value" value="{value}" inputmode="decimal"{required}><select id="{id}-unit" name="{id}__unit" aria-label="{label} unit">{options}</select></span>"#,
                        value = escape_html(&number_value),
                    )
                }
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
                ParameterKind::Unsupported => format!(
                    r#"<input id="{id}" name="{id}" value="{value}" disabled title="Unsupported parameter type">"#,
                    value = escape_html(&display_value),
                ),
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

        assert!(controls.contains(r#"name="width__value" value="2""#));
        assert!(controls.contains(r#"name="width__unit""#));
        assert!(controls.contains(r#"<option value="in" selected>in</option>"#));
        assert!(!controls.contains(r#"value="42 mm""#));
    }

    #[test]
    fn normalizes_split_quantity_form_values() {
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
        let raw = HashMap::from([
            ("width__value".to_owned(), "2".to_owned()),
            ("width__unit".to_owned(), "in".to_owned()),
        ]);

        let values = normalize_form_values(&schema, raw);

        assert_eq!(
            values,
            HashMap::from([("width".to_owned(), "2 in".to_owned())])
        );
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
        assert!(html.contains(r#"control.name === `${parameterId}__value`"#));
        assert!(html.contains(r#"return unit ? `${value} ${unit}` : value"#));
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
    fn configuration_validation_persists_canonical_request_values() {
        let validated = validated_configuration_for_test_values(HashMap::from([(
            "a".to_owned(),
            "01.0".to_owned(),
        )]));

        let validation: Value =
            serde_json::from_str(&configuration_validation_json(&validated).unwrap()).unwrap();

        assert_eq!(validation["submittedValues"]["a"], "1");
        assert_eq!(validation["requestValues"]["a"], "(1/1)");
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
        let onshape = OnshapeClient::new(config::OnshapeConfig {
            base_url: "https://cad.onshape.com".to_owned(),
            access_key: None,
            secret_key: None,
        })
        .unwrap();
        let encoded_configuration = EncodedConfigurationIdentity {
            encoded_id: "enc-123".to_owned(),
            query_param: "configuration=enc-123".to_owned(),
        };
        let preview_request_hash = cache_model::request_hash(
            &onshape
                .build_preview_glb_export_request(
                    &first.onshape,
                    &encoded_configuration,
                    &first.exports.preview_options,
                )
                .identity,
        )
        .unwrap();
        let other_preview_request_hash = cache_model::request_hash(
            &onshape
                .build_preview_glb_export_request(
                    &second.onshape,
                    &encoded_configuration,
                    &second.exports.preview_options,
                )
                .identity,
        )
        .unwrap();
        let download_request_hash = cache_model::request_hash(
            &onshape
                .build_download_export_request(
                    &first.onshape,
                    &encoded_configuration,
                    catalog::DownloadFormat::Step,
                    &first.exports.download_options,
                )
                .identity,
        )
        .unwrap();
        let other_download_request_hash = cache_model::request_hash(
            &onshape
                .build_download_export_request(
                    &second.onshape,
                    &encoded_configuration,
                    catalog::DownloadFormat::Step,
                    &second.exports.download_options,
                )
                .identity,
        )
        .unwrap();

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
        let preview_artifact_hash = preview_artifact_key(
            &source_hash,
            config_hash,
            &preview_options_hash,
            "glb",
            &preview_request_hash,
            "rawhash",
            "posthash",
        )
        .unwrap();
        let other_preview_artifact_hash = preview_artifact_key(
            &source_hash,
            config_hash,
            &preview_options_hash,
            "glb",
            &other_preview_request_hash,
            "rawhash",
            "posthash",
        )
        .unwrap();
        let download_artifact_hash = download_artifact_key(
            &source_hash,
            config_hash,
            &download_options_hash,
            catalog::DownloadFormat::Step.slug(),
            &download_request_hash,
            "rawhash",
            "posthash",
        )
        .unwrap();
        let other_download_artifact_hash = download_artifact_key(
            &source_hash,
            config_hash,
            &download_options_hash,
            catalog::DownloadFormat::Step.slug(),
            &other_download_request_hash,
            "rawhash",
            "posthash",
        )
        .unwrap();

        assert_eq!(preview_artifact_hash, other_preview_artifact_hash);
        assert_eq!(
            preview_work_key(&source_hash, &first, config_hash),
            preview_work_key(&source_hash, &second, config_hash)
        );
        assert_eq!(
            preview_glb_object_key(&preview_artifact_hash),
            preview_glb_object_key(&other_preview_artifact_hash)
        );
        assert_eq!(download_artifact_hash, other_download_artifact_hash);
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
        assert_eq!(preview_request_hash, other_preview_request_hash);
        assert_eq!(download_request_hash, other_download_request_hash);
        assert_eq!(
            export_job_key(&preview_request_hash),
            export_job_key(&other_preview_request_hash)
        );
        assert_eq!(
            export_job_key(&download_request_hash),
            export_job_key(&other_download_request_hash)
        );

        assert_ne!(
            preview_work_key(&source_hash, &first, config_hash),
            preview_lookup_key(&source_hash, &first, config_hash)
        );
        assert_ne!(
            export_job_key(&preview_request_hash),
            preview_work_key(&source_hash, &first, config_hash)
        );
        assert_ne!(
            download_filename(&first, catalog::DownloadFormat::Step),
            download_filename(&second, catalog::DownloadFormat::Step)
        );
        assert_ne!(
            download_object_key(
                &download_artifact_hash,
                &download_filename(&first, catalog::DownloadFormat::Step)
            ),
            download_object_key(
                &other_download_artifact_hash,
                &download_filename(&second, catalog::DownloadFormat::Step)
            )
        );
    }

    #[test]
    fn prefers_active_job_status_over_stale_failed_status() {
        assert!(job_status_priority("running") > job_status_priority("failed"));
        assert!(job_status_priority("queued") > job_status_priority("superseded"));
    }

    #[test]
    fn upload_metadata_verification_checks_length_and_content_type() {
        verify_uploaded_object_metadata(
            &storage::ObjectMetadata {
                content_length: 42,
                content_type: Some("model/gltf-binary".to_owned()),
            },
            "previews/v2/artifact/preview.glb",
            "model/gltf-binary",
            42,
        )
        .unwrap();

        let error = verify_uploaded_object_metadata(
            &storage::ObjectMetadata {
                content_length: 42,
                content_type: Some("application/octet-stream".to_owned()),
            },
            "previews/v2/artifact/preview.glb",
            "model/gltf-binary",
            42,
        )
        .unwrap_err();
        assert!(error.to_string().contains("content type mismatch"));

        let error = verify_uploaded_object_metadata(
            &storage::ObjectMetadata {
                content_length: 41,
                content_type: Some("model/gltf-binary".to_owned()),
            },
            "previews/v2/artifact/preview.glb",
            "model/gltf-binary",
            42,
        )
        .unwrap_err();
        assert!(error.to_string().contains("length mismatch"));
    }

    #[test]
    fn strict_upload_verification_checks_read_back_sha256() {
        let bytes = b"verified-bytes";
        let expected_sha256 = cache_key::hex_sha256(bytes);

        verify_uploaded_object_bytes(
            bytes,
            "artifacts/v2/artifact/demo.step",
            bytes.len() as i64,
            &expected_sha256,
        )
        .unwrap();

        let error = verify_uploaded_object_bytes(
            bytes,
            "artifacts/v2/artifact/demo.step",
            bytes.len() as i64,
            "deadbeef",
        )
        .unwrap_err();
        assert!(error.to_string().contains("sha256 mismatch"));
    }

    #[tokio::test]
    async fn artifact_status_surfaces_upload_failed_artifact_sets() {
        let state = test_state().await;

        let status = artifact_status(
            &state,
            "artifact-lookup".to_owned(),
            None,
            Some(db::ArtifactSetRecord {
                artifact_set_hash: "artifact-set".to_owned(),
                source_hash: "sourcehash".to_owned(),
                config_hash: "confighash".to_owned(),
                options_hash: "optionshash".to_owned(),
                request_hash: Some("requesthash".to_owned()),
                raw_payload_hash: Some("rawhash".to_owned()),
                postprocess_hash: Some("posthash".to_owned()),
                output_kind: "preview_glb".to_owned(),
                format: "glb".to_owned(),
                status: "upload_failed".to_owned(),
                primary_object_key: Some("previews/v2/artifact-set/preview.glb".to_owned()),
                metadata_json: "{}".to_owned(),
                created_at: "2026-01-01T00:00:00.000Z".to_owned(),
                updated_at: "2026-01-01T00:00:01.000Z".to_owned(),
                superseded_at: None,
                superseded_by: None,
                supersession_reason: None,
            }),
            &[],
        )
        .await
        .unwrap();

        assert_eq!(status.status, "upload_failed");
        assert!(status.message.contains("verification failed"));
        assert_eq!(status.artifact_key, "artifact-set");
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

    #[tokio::test]
    async fn export_status_job_keys_ignore_legacy_export_jobs() {
        let state = test_state().await;
        let legacy_work_key = preview_work_key("sourcehash", &test_model(), "confighash");
        state
            .db
            .enqueue_job(&legacy_work_key, "preview_export", "{}")
            .await
            .unwrap();

        let work_keys = export_status_job_keys(
            &state,
            "sourcehash",
            "confighash",
            "optionshash",
            "preview",
            "glb",
        )
        .await
        .unwrap();

        assert!(work_keys.is_empty());

        state
            .db
            .insert_export_request_if_absent(ExportRequestInsert {
                request_hash: "requesthash",
                source_hash: "sourcehash",
                config_hash: "confighash",
                options_hash: "optionshash",
                output_kind: "preview",
                format: "glb",
                endpoint: "createPartStudioExportGltf",
                method: "POST",
                path: "/api/partstudios/d/did/v/mid/e/eid/export/gltf",
                request_json: "{}",
                defaults_policy_version: "v1",
                request_builder_version: "v1",
                status: EXPORT_REQUEST_STATUS_STAGED,
            })
            .await
            .unwrap();
        state
            .db
            .insert_export_request_if_absent(ExportRequestInsert {
                request_hash: "zzz-new-requesthash",
                source_hash: "sourcehash",
                config_hash: "confighash",
                options_hash: "optionshash",
                output_kind: "preview",
                format: "glb",
                endpoint: "createPartStudioExportGltf",
                method: "POST",
                path: "/api/partstudios/d/did/v/mid/e/eid/export/gltf",
                request_json: "{}",
                defaults_policy_version: "v2",
                request_builder_version: "v2",
                status: EXPORT_REQUEST_STATUS_STAGED,
            })
            .await
            .unwrap();

        let work_keys = export_status_job_keys(
            &state,
            "sourcehash",
            "confighash",
            "optionshash",
            "preview",
            "glb",
        )
        .await
        .unwrap();

        assert_eq!(work_keys, vec![export_job_key("zzz-new-requesthash")]);
    }

    #[tokio::test]
    async fn process_next_job_retires_legacy_export_jobs() {
        let state = test_state().await;
        for (work_key, job_kind) in [
            (
                preview_work_key("sourcehash", &test_model(), "confighash"),
                "preview_export",
            ),
            (
                download_work_key(
                    "sourcehash",
                    &test_model(),
                    "confighash",
                    catalog::DownloadFormat::Step,
                ),
                "download_export",
            ),
        ] {
            state
                .db
                .enqueue_job(&work_key, job_kind, "{}")
                .await
                .unwrap();
            assert!(process_next_job(&state).await.unwrap());

            let job = state.db.job(&work_key).await.unwrap().unwrap();
            assert_eq!(job.status, "superseded");
            assert_eq!(
                job.error_summary.as_deref(),
                Some("legacy export jobs are retired after the cache v2 hard cut")
            );
        }
    }

    #[test]
    fn only_non_request_hash_export_jobs_are_retired() {
        assert!(should_retire_legacy_export_job(&db::JobLease {
            work_key: preview_work_key("sourcehash", &test_model(), "confighash"),
            job_kind: "preview_export".to_owned(),
            payload_json: "{}".to_owned(),
            attempt: 1,
            max_attempts: 3,
        }));
        assert!(should_retire_legacy_export_job(&db::JobLease {
            work_key: download_work_key(
                "sourcehash",
                &test_model(),
                "confighash",
                catalog::DownloadFormat::Step,
            ),
            job_kind: "download_export".to_owned(),
            payload_json: "{}".to_owned(),
            attempt: 1,
            max_attempts: 3,
        }));
        assert!(!should_retire_legacy_export_job(&db::JobLease {
            work_key: export_job_key("requesthash"),
            job_kind: "preview_export".to_owned(),
            payload_json: "{}".to_owned(),
            attempt: 1,
            max_attempts: 3,
        }));
        assert!(!should_retire_legacy_export_job(&db::JobLease {
            work_key: parameter_refresh_work_key("sourcehash"),
            job_kind: "parameter_refresh".to_owned(),
            payload_json: r#"{"kind":"parameter_refresh","model_slug":"demo"}"#.to_owned(),
            attempt: 1,
            max_attempts: 3,
        }));
    }

    #[tokio::test]
    async fn preview_status_prefers_current_request_hash_over_old_ready_artifact() {
        let state = test_state().await;
        let model = test_model();
        let source_hash = resolved_source_hash_for_test_model(&model);
        let validated = validated_configuration_for_test_values(HashMap::from([(
            "a".to_owned(),
            "1".to_owned(),
        )]));
        let config_hash = configuration_hash(&source_hash, &validated).unwrap();
        seed_test_source_resolution(&state, &model, &source_hash).await;
        state
            .db
            .upsert_configuration_encoding(db::ConfigurationEncodingUpsert {
                source_hash: &source_hash,
                config_hash: &config_hash,
                encoded_id: "encoded-1",
                query_param: "configuration=encoded-1",
                request_json: r#"{"parameters":[{"parameterId":"a","parameterValue":"1"}]}"#,
                response_json: r#"{"encodedId":"encoded-1","queryParam":"configuration=encoded-1"}"#,
            })
            .await
            .unwrap();
        let current_request = state.onshape.build_preview_glb_export_request(
            &model.onshape,
            &EncodedConfigurationIdentity {
                encoded_id: "encoded-1".to_owned(),
                query_param: "configuration=encoded-1".to_owned(),
            },
            &model.exports.preview_options,
        );
        let current_request_hash = cache_model::request_hash(&current_request.identity).unwrap();
        let queued_work_key = export_job_key(&current_request_hash);
        state
            .db
            .enqueue_job(&queued_work_key, "preview_export", "{}")
            .await
            .unwrap();
        state
            .db
            .upsert_artifact(db::ArtifactUpsert {
                artifact_key: "old-ready-artifact",
                model_slug: &model.slug,
                config_hash: &config_hash,
                output_kind: "preview_glb",
                format: "glb",
                object_key: "previews/v2/old-ready-artifact/preview.glb",
                content_type: "model/gltf-binary",
                byte_len: 42,
                sha256: "old-sha",
                producing_job_key: Some("work-v2:export:old-requesthash"),
                source_hash: &source_hash,
                options_hash: &preview_options_hash(&model),
                request_hash: Some("old-requesthash"),
                raw_payload_hash: Some("rawhash-old"),
                postprocess_hash: Some("posthash-old"),
                parameter_schema_version: SCHEMA_VERSION.into(),
                config_values_json: "{}",
            })
            .await
            .unwrap();

        let Json(status) = preview_status(
            State(state.clone()),
            Path((model.slug.clone(), config_hash.clone())),
        )
        .await
        .unwrap();

        assert_eq!(status.status, "queued");
        assert_eq!(status.job_id.as_deref(), Some(queued_work_key.as_str()));
        assert_eq!(status.public_url, None);
    }

    #[tokio::test]
    async fn enqueue_preview_dedupes_identical_request_hash() {
        let state = test_state().await;
        let model = test_model();
        let source_hash = resolved_source_hash_for_test_model(&model);
        let validated = validated_configuration_for_test_values(HashMap::from([(
            "a".to_owned(),
            "1".to_owned(),
        )]));
        let config_hash = configuration_hash(&source_hash, &validated).unwrap();
        seed_test_source_resolution(&state, &model, &source_hash).await;
        state
            .db
            .upsert_configuration_encoding(db::ConfigurationEncodingUpsert {
                source_hash: &source_hash,
                config_hash: &config_hash,
                encoded_id: "encoded-1",
                query_param: "configuration=encoded-1",
                request_json: r#"{"parameters":[{"parameterId":"a","parameterValue":"1"}]}"#,
                response_json: r#"{"encodedId":"encoded-1","queryParam":"configuration=encoded-1"}"#,
            })
            .await
            .unwrap();

        assert!(enqueue_preview(&state, &model, &validated).await.unwrap());
        assert!(!enqueue_preview(&state, &model, &validated).await.unwrap());

        let prepared =
            prepare_preview_export(&state, &model, &source_hash, &validated, &config_hash)
                .await
                .unwrap();
        let job = state
            .db
            .job(&export_job_key(&prepared.request_hash))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(job.status, "queued");
    }

    #[tokio::test]
    async fn superseding_artifact_marks_only_the_recorded_job() {
        let state = test_state().await;
        let work_key = export_job_key("requesthash");
        state
            .db
            .enqueue_job(&work_key, "preview_export", "{}")
            .await
            .unwrap();
        let lease = state.db.claim_next_job(30).await.unwrap().unwrap();
        state
            .db
            .finish_job(&work_key, lease.attempt, "ready", None)
            .await
            .unwrap();
        state
            .db
            .upsert_artifact(db::ArtifactUpsert {
                artifact_key: "artifact-set",
                model_slug: "demo",
                config_hash: "confighash",
                output_kind: "preview_glb",
                format: "glb",
                object_key: "previews/v2/artifact-set/preview.glb",
                content_type: "model/gltf-binary",
                byte_len: 42,
                sha256: "sha256",
                producing_job_key: Some(&work_key),
                source_hash: "sourcehash",
                options_hash: "optionshash",
                request_hash: Some("requesthash"),
                raw_payload_hash: Some("rawhash"),
                postprocess_hash: Some("posthash"),
                parameter_schema_version: SCHEMA_VERSION.into(),
                config_values_json: "{}",
            })
            .await
            .unwrap();

        let artifact = state.db.artifact("artifact-set").await.unwrap().unwrap();
        supersede_published_artifact(&state, &artifact)
            .await
            .unwrap();

        assert!(state.db.artifact("artifact-set").await.unwrap().is_none());
        assert_eq!(
            state.db.job(&work_key).await.unwrap().unwrap().status,
            "superseded"
        );
    }

    #[test]
    fn preserves_direct_glb_preview_exports() {
        let bytes = valid_glb();
        let artifact = preview_artifact_from_onshape_bytes(bytes.clone()).unwrap();

        assert_eq!(artifact.logical_path, "preview.glb");
        assert_eq!(artifact.content_type, "model/gltf-binary");
        assert_eq!(artifact.bytes, bytes);
    }

    #[test]
    fn preserves_direct_gltf_preview_exports() {
        let bytes = br#"{"asset":{"version":"2.0"}}"#.to_vec();
        let artifact = preview_artifact_from_onshape_bytes(bytes.clone()).unwrap();

        assert_eq!(artifact.logical_path, "preview.gltf");
        assert_eq!(artifact.content_type, "model/gltf+json");
        assert_eq!(artifact.bytes, bytes);
        assert!(artifact.sidecars.is_empty());
    }

    #[test]
    fn extracts_single_glb_from_zipped_preview_exports() {
        let glb = valid_glb();
        let bytes = test_zip(&[
            ("scene.gltf", br#"{"asset":{"version":"2.0"}}"#.as_slice()),
            ("scene.bin", b"loose buffer".as_slice()),
            ("preview.glb", glb.as_slice()),
        ]);

        let artifact = preview_artifact_from_onshape_bytes(bytes).unwrap();

        assert_eq!(artifact.logical_path, "preview.glb");
        assert_eq!(artifact.content_type, "model/gltf-binary");
        assert_eq!(artifact.bytes, glb);
        assert!(artifact.sidecars.is_empty());
    }

    #[test]
    fn rejects_invalid_direct_glb_preview_exports() {
        let error = preview_artifact_from_onshape_bytes(b"glTFbytes".to_vec()).unwrap_err();

        assert!(error.to_string().contains("validating direct GLB"));
    }

    #[test]
    fn extracts_gltf_asset_set_from_zipped_preview_exports() {
        let bytes = test_zip(&[
            (
                "scene.gltf",
                br#"{"asset":{"version":"2.0"},"buffers":[{"uri":"scene.bin"}]}"#.as_slice(),
            ),
            ("scene.bin", b"loose buffer".as_slice()),
        ]);

        let artifact = preview_artifact_from_onshape_bytes(bytes).unwrap();

        assert_eq!(artifact.logical_path, "scene.gltf");
        assert_eq!(artifact.content_type, "model/gltf+json");
        assert_eq!(artifact.sidecars.len(), 1);
        assert!(
            artifact
                .sidecars
                .iter()
                .any(|sidecar| sidecar.logical_path == "scene.bin")
        );
    }

    #[test]
    fn extracts_nested_gltf_asset_set_from_zipped_preview_exports() {
        let bytes = test_zip(&[
            (
                "scene/model.gltf",
                br#"{"asset":{"version":"2.0"},"buffers":[{"uri":"scene.bin"}],"images":[{"uri":"textures/albedo.png"}]}"#.as_slice(),
            ),
            ("scene/scene.bin", b"loose buffer".as_slice()),
            ("scene/textures/albedo.png", b"png".as_slice()),
        ]);

        let artifact = preview_artifact_from_onshape_bytes(bytes).unwrap();

        assert_eq!(artifact.logical_path, "scene/model.gltf");
        assert!(
            artifact
                .sidecars
                .iter()
                .any(|sidecar| sidecar.logical_path == "scene/scene.bin")
        );
        assert!(
            artifact
                .sidecars
                .iter()
                .any(|sidecar| sidecar.logical_path == "scene/textures/albedo.png")
        );
    }

    #[test]
    fn rejects_gltf_zip_with_missing_referenced_sidecars() {
        let bytes = test_zip(&[(
            "scene.gltf",
            br#"{"asset":{"version":"2.0"},"buffers":[{"uri":"scene.bin"}]}"#.as_slice(),
        )]);

        let error = preview_artifact_from_onshape_bytes(bytes).unwrap_err();

        assert!(
            error
                .to_string()
                .contains("missing referenced sidecar asset: scene.bin")
        );
    }

    #[test]
    fn rejects_gltf_zip_with_unsafe_referenced_sidecar_paths() {
        let bytes = test_zip(&[
            (
                "scene/model.gltf",
                br#"{"asset":{"version":"2.0"},"buffers":[{"uri":"../scene.bin"}]}"#.as_slice(),
            ),
            ("scene.bin", b"loose buffer".as_slice()),
        ]);

        let error = preview_artifact_from_onshape_bytes(bytes).unwrap_err();

        assert!(
            error
                .to_string()
                .contains("validating zipped glTF preview JSON")
        );
    }

    #[test]
    fn rejects_gltf_zip_with_data_uri_sidecars() {
        let bytes = test_zip(&[(
            "scene.gltf",
            br#"{"asset":{"version":"2.0"},"images":[{"uri":"data:image/png;base64,AAAA"}]}"#
                .as_slice(),
        )]);

        let error = preview_artifact_from_onshape_bytes(bytes).unwrap_err();

        assert!(
            error
                .to_string()
                .contains("validating zipped glTF preview JSON")
        );
    }

    #[test]
    fn rejects_multiple_gltf_zipped_preview_exports() {
        let bytes = test_zip(&[
            ("first.gltf", br#"{"asset":{"version":"2.0"}}"#.as_slice()),
            ("second.gltf", br#"{"asset":{"version":"2.0"}}"#.as_slice()),
            ("scene.bin", b"loose buffer".as_slice()),
        ]);

        let error = preview_artifact_from_onshape_bytes(bytes).unwrap_err();

        assert!(error.to_string().contains("multiple glTF files"));
    }

    #[test]
    fn rejects_unsafe_gltf_zip_asset_paths() {
        let bytes = test_zip(&[
            ("scene.gltf", br#"{"asset":{"version":"2.0"}}"#.as_slice()),
            ("../scene.bin", b"loose buffer".as_slice()),
        ]);

        let error = preview_artifact_from_onshape_bytes(bytes).unwrap_err();

        assert!(error.to_string().contains("not safe"));
    }

    #[test]
    fn rejects_multiple_glbs_in_zipped_preview_exports() {
        let first = valid_glb();
        let second = valid_glb();
        let bytes = test_zip(&[
            ("first.glb", first.as_slice()),
            ("second.glb", second.as_slice()),
        ]);

        let error = preview_artifact_from_onshape_bytes(bytes).unwrap_err();

        assert!(error.to_string().contains("multiple GLB files"));
    }

    #[test]
    fn rejects_invalid_zipped_glb_preview_exports() {
        let bytes = test_zip(&[("preview.glb", b"glTFzip".as_slice())]);

        let error = preview_artifact_from_onshape_bytes(bytes).unwrap_err();

        assert!(error.to_string().contains("validating zipped GLB"));
    }

    #[test]
    fn raw_payload_hash_matches_exact_download_bytes() {
        let first = cache_key::hex_sha256(b"abc");
        let second = cache_key::hex_sha256(b"abcd");

        assert_ne!(first, second);
    }

    #[test]
    fn verify_raw_payload_bytes_rejects_hash_mismatch() {
        let error = verify_raw_payload_bytes(&cache_key::hex_sha256(b"abc"), b"abcd").unwrap_err();

        assert!(error.to_string().contains("sha256 mismatch"));
    }

    #[test]
    fn raw_payload_object_key_is_content_addressed() {
        assert_eq!(
            raw_payload_object_key("abcdef"),
            "onshape/raw/v2/ab/abcdef/payload.bin"
        );
    }

    #[test]
    fn zip_inventory_records_safe_entries() {
        let json = zip_inventory_json(&test_zip(&[("scene.gltf", b"{}"), ("scene.bin", b"1234")]))
            .unwrap()
            .unwrap();

        assert_eq!(
            json,
            r#"[{"path":"scene.gltf","byteLen":2},{"path":"scene.bin","byteLen":4}]"#
        );
    }

    #[test]
    fn zip_inventory_rejects_unsafe_paths() {
        let error = zip_inventory_json(&test_zip(&[("../scene.bin", b"1234")])).unwrap_err();

        assert!(error.to_string().contains("not safe"));
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
    fn parses_default_and_explicit_serve_commands() {
        let cli = Cli::try_parse_from(["onshape-export"]).unwrap();
        assert!(cli.command.is_none());

        let cli = Cli::try_parse_from(["onshape-export", "serve"]).unwrap();
        assert!(matches!(cli.command, Some(CliCommand::Serve)));
    }

    #[test]
    fn parses_catalog_list_json_command() {
        let cli = Cli::try_parse_from(["onshape-export", "catalog", "list", "--json"]).unwrap();

        let Some(CliCommand::Catalog {
            command: CatalogCommand::List(args),
        }) = cli.command
        else {
            panic!("expected catalog list command");
        };

        assert_eq!(args.output_format(), OutputFormat::Json);
    }

    #[test]
    fn parses_deploy_maintenance_options() {
        let cli = Cli::try_parse_from([
            "onshape-export",
            "ops",
            "deploy-maintenance",
            "--reset-generated-state",
            "--reset-catalog-from-seed",
            "--fresh-database",
            "--catalog-seed",
            "catalog/custom.json",
            "--backup-label",
            "123-abc",
            "--backup-prefix",
            "sqlite/manual",
            "--confirm",
            "WIPE",
        ])
        .unwrap();

        let Some(CliCommand::Ops {
            command: OpsCommand::DeployMaintenance(args),
        }) = cli.command
        else {
            panic!("expected deploy maintenance command");
        };
        let options = DeployMaintenanceOptions::from(args);

        assert!(options.reset_generated_state);
        assert!(options.reset_catalog_from_seed);
        assert!(options.fresh_database);
        assert_eq!(options.catalog_seed, "catalog/custom.json");
        assert_eq!(options.backup_label.as_deref(), Some("123-abc"));
        assert_eq!(options.backup_prefix, "sqlite/manual");
        ensure_destructive_options_confirmed(&options).unwrap();
    }

    #[test]
    fn deploy_maintenance_destructive_options_require_confirmation() {
        let cli = Cli::try_parse_from([
            "onshape-export",
            "ops",
            "deploy-maintenance",
            "--fresh-database",
        ])
        .unwrap();
        let Some(CliCommand::Ops {
            command: OpsCommand::DeployMaintenance(args),
        }) = cli.command
        else {
            panic!("expected deploy maintenance command");
        };
        let options = DeployMaintenanceOptions::from(args);

        let error = ensure_destructive_options_confirmed(&options).unwrap_err();

        assert!(error.to_string().contains("--confirm WIPE"));
    }

    #[test]
    fn parses_sqlite_database_paths() {
        assert_eq!(
            sqlite_database_path("sqlite:///data/onshape-export.db?mode=rwc").unwrap(),
            PathBuf::from("/data/onshape-export.db")
        );
        assert_eq!(
            sqlite_database_path("sqlite://onshape-export.db?mode=rwc").unwrap(),
            PathBuf::from("onshape-export.db")
        );
        assert!(sqlite_database_path("sqlite::memory:").is_none());
    }

    #[test]
    fn parses_generation_selectors() {
        let cli = Cli::try_parse_from([
            "onshape-export",
            "exports",
            "generate",
            "--all",
            "--all",
            "--all-parameter-sets",
        ])
        .unwrap();

        let Some(CliCommand::Exports {
            command:
                ExportsCommand::Generate(GenerateExportArgs {
                    selector,
                    format,
                    parameter_selector,
                }),
        }) = cli.command
        else {
            panic!("expected exports generate command");
        };

        assert_eq!(selector, "--all");
        assert_eq!(format, "--all");
        assert_eq!(parameter_selector.as_deref(), Some("--all-parameter-sets"));
    }

    #[test]
    fn parses_failure_retry_selectors() {
        let cli = Cli::try_parse_from(["onshape-export", "failures", "retry"]).unwrap();
        let Some(CliCommand::Failures {
            command: FailuresCommand::Retry(args),
        }) = cli.command
        else {
            panic!("expected failures retry command");
        };
        assert_eq!(args.selector(), FailureRetrySelector::All);

        let cli = Cli::try_parse_from([
            "onshape-export",
            "failures",
            "retry",
            "work-v2:preview:demo:abc",
        ])
        .unwrap();
        let Some(CliCommand::Failures {
            command: FailuresCommand::Retry(args),
        }) = cli.command
        else {
            panic!("expected failures retry command");
        };
        assert_eq!(
            args.selector(),
            FailureRetrySelector::WorkKey("work-v2:preview:demo:abc")
        );

        let cli = Cli::try_parse_from([
            "onshape-export",
            "failures",
            "retry",
            "--kind",
            "preview_export",
        ])
        .unwrap();
        let Some(CliCommand::Failures {
            command: FailuresCommand::Retry(args),
        }) = cli.command
        else {
            panic!("expected failures retry command");
        };
        assert_eq!(
            args.selector(),
            FailureRetrySelector::Kind("preview_export")
        );

        assert!(Cli::try_parse_from(["onshape-export", "failures", "retry", "--missing"]).is_err());
        assert!(
            Cli::try_parse_from(["onshape-export", "failures", "retry", "one", "two"]).is_err()
        );
    }

    #[test]
    fn parses_prune_options() {
        let cli = Cli::try_parse_from([
            "onshape-export",
            "artifacts",
            "prune",
            "--all",
            "--older-than-days",
            "30",
            "--dry-run",
        ])
        .unwrap();

        let Some(CliCommand::Artifacts {
            command:
                ArtifactsCommand::Prune(PruneArgs {
                    selector,
                    older_than_days,
                    dry_run,
                }),
        }) = cli.command
        else {
            panic!("expected artifacts prune command");
        };

        assert_eq!(selector, "--all");
        assert_eq!(
            PruneOptions::new(older_than_days, dry_run).unwrap(),
            PruneOptions {
                older_than_days: 30,
                dry_run: true,
            }
        );
        assert!(PruneOptions::new(0, false).is_err());
    }

    async fn test_state() -> AppState {
        let directory = tempfile::tempdir().unwrap();
        let database_url = format!(
            "sqlite://{}?mode=rwc",
            directory.path().join("test.db").display()
        );
        let db = Database::connect(&database_url).await.unwrap();
        std::mem::forget(directory);
        let catalog = test_catalog();
        db.replace_catalog(&catalog).await.unwrap();
        let storage = StorageClient::new(crate::config::StorageConfig {
            bucket: "test-bucket".to_owned(),
            endpoint_url: Some("http://127.0.0.1:9000".to_owned()),
            region: "auto".to_owned(),
            access_key_id: None,
            secret_access_key: None,
            public_base_url: Some("https://cdn.example.com".to_owned()),
            force_path_style: true,
        })
        .await
        .unwrap();
        let onshape = OnshapeClient::new(crate::config::OnshapeConfig {
            base_url: "https://cad.onshape.com".to_owned(),
            access_key: None,
            secret_key: None,
        })
        .unwrap();

        AppState {
            db,
            onshape,
            storage,
        }
    }

    fn test_catalog() -> Catalog {
        serde_json::from_value(serde_json::json!({
            "catalogSchemaVersion": catalog::CATALOG_SCHEMA_VERSION,
            "models": [test_model()],
        }))
        .unwrap()
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

    async fn seed_test_source_resolution(
        state: &AppState,
        model: &catalog::Model,
        source_hash: &str,
    ) {
        state
            .db
            .upsert_source_resolution(db::SourceResolutionUpsert {
                source_hash,
                model_slug: &model.slug,
                document_id: &model.onshape.document_id,
                version_id: &model.onshape.version_id,
                microversion_id: "mid",
                element_id: &model.onshape.element_id,
                element_kind: model.onshape.element_kind.key(),
                link_document_id: model.onshape.link_document_id.as_deref(),
                diagnostics_json: r#"{"microversionId":"mid"}"#,
            })
            .await
            .unwrap();
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
