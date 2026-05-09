mod config;
mod http;
mod registry;
mod schema;
mod storage;
mod validation;

use std::sync::Arc;

use config::Config;
use storage::Store;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::from_env()?;
    let listener = tokio::net::TcpListener::bind(config.bind).await?;
    let store = Arc::new(Store::open(&config.database_path)?);
    store.apply_retention(config.raw_retention_days, config.dedupe_retention_days)?;
    spawn_retention_worker(
        Arc::clone(&store),
        config.raw_retention_days,
        config.dedupe_retention_days,
    );

    eprintln!(
        "posthaste telemetry ingest listening on {}",
        listener.local_addr()?
    );

    axum::serve(listener, http::router(config, store)).await?;
    Ok(())
}

fn spawn_retention_worker(store: Arc<Store>, raw_retention_days: i64, dedupe_retention_days: i64) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(60 * 60));
        loop {
            interval.tick().await;
            if let Err(error) = store.apply_retention(raw_retention_days, dedupe_retention_days) {
                eprintln!("telemetry retention cleanup failed: {error}");
            }
        }
    });
}
