mod catalog;
mod config;
mod db;
mod onshape;
mod parameters;
mod storage;

use std::{collections::HashMap, sync::Arc};

use anyhow::Context;
use axum::{
    Form, Router,
    extract::{Path, State},
    http::StatusCode,
    response::{Html, IntoResponse, Response},
    routing::{get, post},
};
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

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();

    let config = Config::from_env()?;
    let catalog = Arc::new(Catalog::load(&config.catalog_path).context("loading catalog")?);
    let db = Database::connect(&config.database_url)
        .await
        .context("connecting to database")?;
    let storage = StorageClient::new(config.storage.clone()).await?;
    let onshape = OnshapeClient::new(config.onshape.clone())?;

    let state = AppState {
        catalog,
        db,
        onshape,
        storage,
    };

    let listener = tokio::net::TcpListener::bind(config.bind_addr)
        .await
        .context("binding listener")?;
    tracing::info!(address = %listener.local_addr()?, "listening");

    axum::serve(listener, app(state))
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("serving app")
}

fn app(state: AppState) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/healthz", get(healthz))
        .route(
            "/models/{slug}",
            get(model_page).post(validate_model_config),
        )
        .route("/models/{slug}/preview", post(generate_preview))
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

    Ok(render_model_html(model, &parameter_controls, &preview))
}

fn render_model_html(
    model: &catalog::Model,
    parameter_controls: &str,
    preview: &str,
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
    </form>
    <section>
      <h2>Preview</h2>
      {preview}
    </section>
  </main>
</body>
</html>"#,
        slug = escape_html(&model.slug),
        name = escape_html(&model.name),
        description = escape_html(&model.description),
        parameter_controls = parameter_controls,
        preview = preview,
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
                return Ok(render_model_html(model, &parameter_controls, &preview));
            }
        };
    let config_hash = configuration_hash(&validated.values)?;
    let artifact_key = preview_artifact_key(model, &config_hash);

    if let Some(record) = state.db.artifact(&artifact_key).await? {
        let preview = render_preview_result(&state, &record.object_key);
        return Ok(render_model_html(model, &parameter_controls, &preview));
    }

    let work_key = artifact_key.clone();
    if !state.db.try_start_job(&work_key, "preview_glb").await? {
        return Ok(render_model_html(
            model,
            &parameter_controls,
            "<p>Preview generation is already running. Reload this page shortly.</p>",
        ));
    }

    let result = refresh_preview(
        &state,
        model,
        &validated.values,
        &config_hash,
        &artifact_key,
    )
    .await;
    match result {
        Ok(object_key) => {
            state.db.finish_job(&work_key, "ready", None).await?;
            let preview = render_preview_result(&state, &object_key);
            Ok(render_model_html(model, &parameter_controls, &preview))
        }
        Err(error) => {
            let summary = error.to_string();
            state
                .db
                .finish_job(&work_key, "failed", Some(&summary))
                .await?;
            Err(error.into())
        }
    }
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

    let work_key = format!("parameter-refresh:{}", model.slug);
    if !state
        .db
        .try_start_job(&work_key, "parameter_refresh")
        .await?
    {
        return Ok(None);
    }

    let result = refresh_parameters(state, model).await;
    match result {
        Ok(schema) => {
            state.db.finish_job(&work_key, "ready", None).await?;
            Ok(Some(schema))
        }
        Err(error) => {
            let summary = error.to_string();
            state
                .db
                .finish_job(&work_key, "failed", Some(&summary))
                .await?;
            Err(error.into())
        }
    }
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
    let artifact_key = preview_artifact_key(model, &config_hash);

    match state.db.artifact(&artifact_key).await? {
        Some(record) => Ok(render_preview_viewer(state, &record.object_key)),
        None => Ok("<p>No cached preview for the default parameters yet.</p>".to_owned()),
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
    Ok(object_key)
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
    let mut keys = values.keys().collect::<Vec<_>>();
    keys.sort();
    for key in keys {
        object.insert(key.clone(), Value::String(values[key].clone()));
    }
    let canonical = serde_json::to_vec(&Value::Object(object))?;
    Ok(hex_sha256(&canonical))
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
}
