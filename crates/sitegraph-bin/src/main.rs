//! SiteGraph server entrypoint.
//!
//! Reads config from the environment, applies migrations, then serves the API.
//! Config layer stays minimal in the skeleton (WP1.0); `figment` + typed config
//! is a later upgrade per the dev plan.

use anyhow::{Context, Result};
use sitegraph_api::rate_limit::RateLimiter;
use sitegraph_api::{router, AppState};
use sitegraph_db::Db;
use std::env;
use std::net::SocketAddr;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,sqlx=warn".into()),
        )
        .init();

    let database_url = env::var("DATABASE_URL")
        .context("DATABASE_URL must be set (see deploy/.env.example)")?;
    let bind = env::var("BIND").unwrap_or_else(|_| "0.0.0.0:8080".into());
    // Dev default keeps the skeleton runnable; production MUST set JWT_SECRET.
    let jwt_secret = env::var("JWT_SECRET").unwrap_or_else(|_| {
        tracing::warn!("JWT_SECRET not set — using insecure dev default");
        "dev-insecure-secret-change-me".into()
    });

    let db = Db::connect(&database_url)
        .await
        .context("failed to connect to database")?;
    db.migrate().await.context("failed to run migrations")?;
    tracing::info!("migrations applied");

    sitegraph_api::bootstrap_demo(&db)
        .await
        .context("demo bootstrap failed")?;

    let app = router(AppState {
        db,
        jwt_secret,
        // Sensible defaults; tune per deployment (WP1.6).
        delivery_limiter: Arc::new(RateLimiter::new(120, 60)), // 120 req / min / token
        login_limiter: Arc::new(RateLimiter::new(10, 60)),     // 10 attempts / min / IP
    });

    let listener = tokio::net::TcpListener::bind(&bind)
        .await
        .with_context(|| format!("failed to bind {bind}"))?;
    tracing::info!("SiteGraph listening on http://{bind}");
    // ConnectInfo provides the client IP to the login rate limiter.
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;
    Ok(())
}
