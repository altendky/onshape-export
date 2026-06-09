mod catalog;
mod config;
mod db;
mod onshape;
mod storage;

use std::sync::Arc;

use anyhow::Context;
use axum::{
    Router,
    extract::{Path, State},
    http::StatusCode,
    response::{Html, IntoResponse, Response},
    routing::get,
};
use tower_http::trace::TraceLayer;
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

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
        .route("/models/{slug}", get(model_page))
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
    <p>Parameter discovery and export generation will be added in the next phases.</p>
  </main>
</body>
</html>"#,
        name = escape_html(&model.name),
        description = escape_html(&model.description),
    )))
}

#[derive(Debug, thiserror::Error)]
enum AppError {
    #[error("not found")]
    NotFound,
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        match self {
            Self::NotFound => (StatusCode::NOT_FOUND, "not found\n").into_response(),
            Self::Database(error) => {
                tracing::error!(%error, "database error");
                (StatusCode::INTERNAL_SERVER_ERROR, "internal server error\n").into_response()
            }
        }
    }
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
