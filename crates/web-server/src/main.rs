mod api;
mod sanitize;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use axum::routing::{get, post};
use axum::Router;
use dotenvy::dotenv;
use mail_domain::{AccountId, DomainEvent, MailService};
use mail_jmap::{LiveJmapGateway, MockJmapGateway};
use mail_store::DatabaseStore;
use tokio::sync::broadcast;
use tower_http::cors::{AllowOrigin, CorsLayer};

pub struct AppState {
    pub service: Arc<MailService>,
    pub event_sender: broadcast::Sender<DomainEvent>,
}

impl AppState {
    pub fn publish_events(&self, events: &[DomainEvent]) {
        for event in events {
            let _ = self.event_sender.send(event.clone());
        }
    }
}

#[tokio::main]
async fn main() {
    dotenv().ok();

    let account_id =
        AccountId(std::env::var("MAIL_ACCOUNT_ID").unwrap_or_else(|_| "primary".to_string()));
    let db_path = PathBuf::from(
        std::env::var("MAIL_DB_PATH").unwrap_or_else(|_| "data/mail.sqlite".to_string()),
    );
    let data_root =
        PathBuf::from(std::env::var("MAIL_DATA_ROOT").unwrap_or_else(|_| "data".to_string()));
    let store =
        Arc::new(DatabaseStore::open(&db_path, &data_root).expect("failed to initialize store"));

    let service = if let Some(gateway) = build_gateway().await {
        Arc::new(MailService::new(store).with_gateway(&account_id, gateway))
    } else {
        Arc::new(MailService::new(store))
    };

    let (event_sender, _) = broadcast::channel(512);
    let state = Arc::new(AppState {
        service,
        event_sender,
    });

    if state
        .service
        .sync_account(&account_id)
        .await
        .map(|events| {
            state.publish_events(&events);
        })
        .is_err()
    {
        if let Err(error) = state.service.record_sync_failure(
            &account_id,
            "startup_sync_failed",
            "initial sync failed; serving cached data only",
        ) {
            eprintln!("failed to record startup sync failure: {error}");
        }
    }

    if has_gateway() {
        let sync_state = state.clone();
        let sync_account = account_id.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(60));
            interval.tick().await;
            loop {
                interval.tick().await;
                match sync_state.service.sync_account(&sync_account).await {
                    Ok(events) => sync_state.publish_events(&events),
                    Err(error) => {
                        eprintln!("background sync failed: {error}");
                        match sync_state.service.record_sync_failure(
                            &sync_account,
                            error.code(),
                            &error.to_string(),
                        ) {
                            Ok(event) => sync_state.publish_events(&[event]),
                            Err(record_error) => {
                                eprintln!("failed to persist sync failure event: {record_error}")
                            }
                        }
                    }
                }
            }
        });
    }

    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::exact(
            std::env::var("MAIL_CORS_ORIGIN")
                .unwrap_or_else(|_| "http://localhost:5173".to_string())
                .parse()
                .expect("invalid CORS origin"),
        ))
        .allow_methods(tower_http::cors::Any)
        .allow_headers(tower_http::cors::Any);

    let app = Router::new()
        .route(
            "/v1/accounts/{account_id}/mailboxes",
            get(api::list_mailboxes),
        )
        .route(
            "/v1/accounts/{account_id}/messages",
            get(api::list_messages),
        )
        .route(
            "/v1/accounts/{account_id}/messages/{message_id}",
            get(api::get_message),
        )
        .route(
            "/v1/accounts/{account_id}/threads/{thread_id}",
            get(api::get_thread),
        )
        .route(
            "/v1/accounts/{account_id}/commands/messages/{message_id}:set-keywords",
            post(api::set_keywords),
        )
        .route(
            "/v1/accounts/{account_id}/commands/messages/{message_id}:add-to-mailbox",
            post(api::add_to_mailbox),
        )
        .route(
            "/v1/accounts/{account_id}/commands/messages/{message_id}:remove-from-mailbox",
            post(api::remove_from_mailbox),
        )
        .route(
            "/v1/accounts/{account_id}/commands/messages/{message_id}:replace-mailboxes",
            post(api::replace_mailboxes),
        )
        .route(
            "/v1/accounts/{account_id}/commands/messages/{message_id}:destroy",
            post(api::destroy_message),
        )
        .route(
            "/v1/accounts/{account_id}/commands/sync",
            post(api::trigger_sync),
        )
        .route("/v1/events", get(api::stream_events))
        .layer(cors)
        .with_state(state);

    let bind_address = std::env::var("MAIL_BIND").unwrap_or_else(|_| "0.0.0.0:3001".to_string());
    let listener = tokio::net::TcpListener::bind(&bind_address)
        .await
        .expect("failed to bind daemon listener");
    println!("mail-daemon listening on http://{bind_address}");
    axum::serve(listener, app)
        .await
        .expect("daemon server failed");
}

async fn build_gateway() -> Option<Arc<dyn mail_domain::MailGateway>> {
    match std::env::var("MAIL_DRIVER")
        .unwrap_or_else(|_| "none".to_string())
        .as_str()
    {
        "mock" => Some(Arc::new(MockJmapGateway::default())),
        "jmap" => {
            let url = std::env::var("JMAP_URL").ok()?;
            let username = std::env::var("JMAP_USERNAME").ok()?;
            let password = std::env::var("JMAP_PASSWORD").ok()?;
            match LiveJmapGateway::connect(&url, &username, &password).await {
                Ok(gateway) => Some(Arc::new(gateway)),
                Err(error) => {
                    eprintln!("failed to initialize JMAP gateway: {error}");
                    None
                }
            }
        }
        _ => None,
    }
}

fn has_gateway() -> bool {
    matches!(
        std::env::var("MAIL_DRIVER")
            .unwrap_or_else(|_| "none".to_string())
            .as_str(),
        "mock" | "jmap"
    )
}
