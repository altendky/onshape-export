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
    routing::get,
};
use tower_http::trace::TraceLayer;
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

use crate::parameters::{ParameterKind, ParameterSchema, normalize_configuration, validate_values};
use crate::{
    catalog::Catalog, config::Config, db::Database, onshape::OnshapeClient, storage::StorageClient,
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

    Ok(Html(format!(
        r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
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
    </form>
  </main>
</body>
</html>"#,
        name = escape_html(&model.name),
        description = escape_html(&model.description),
        parameter_controls = parameter_controls,
    )))
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
                            let selected = (option.value == default_value)
                                .then_some(" selected")
                                .unwrap_or("");
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
}
