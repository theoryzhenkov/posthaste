mod api;
mod config;
mod sanitize;
mod secret;
mod supervisor;

use std::sync::Arc;
use std::time::Duration;

use axum::routing::{get, post};
use axum::Router;
use dotenvy::dotenv;
use mail_domain::{DomainEvent, MailService, MailStore, SecretStore};
use mail_store::DatabaseStore;
use tokio::sync::broadcast;
use tower_http::cors::{AllowOrigin, CorsLayer};

use crate::config::{load_bootstrap_config, seed_accounts, seed_app_settings, BootstrapSeedConfig};
use crate::secret::SystemSecretStore;
use crate::supervisor::AccountSupervisor;

pub struct AppState {
    pub service: Arc<MailService>,
    pub store: Arc<dyn MailStore>,
    pub secret_store: Arc<dyn SecretStore>,
    pub supervisor: Arc<AccountSupervisor>,
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

    let (bootstrap, seed) = load_bootstrap_config().expect("failed to load bootstrap config");
    let db_path = bootstrap.data_root.join("mail.sqlite");
    let store: Arc<dyn MailStore> = Arc::new(
        DatabaseStore::open(&db_path, &bootstrap.data_root).expect("failed to initialize store"),
    );

    seed_store_if_empty(&store, &seed).expect("failed to seed configuration store");

    let service = Arc::new(MailService::new(store.clone()));
    let (event_sender, _) = broadcast::channel(512);
    let secret_store: Arc<dyn SecretStore> = Arc::new(SystemSecretStore);
    let supervisor = Arc::new(AccountSupervisor::new(
        service.clone(),
        store.clone(),
        secret_store.clone(),
        event_sender.clone(),
        Duration::from_secs(bootstrap.poll_interval_seconds),
    ));

    for account in store
        .list_accounts()
        .expect("failed to load account configuration")
    {
        supervisor.start_account(&account).await;
    }

    let state = Arc::new(AppState {
        service,
        store,
        secret_store,
        supervisor,
        event_sender,
    });

    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::exact(
            bootstrap.cors_origin.parse().expect("invalid CORS origin"),
        ))
        .allow_methods(tower_http::cors::Any)
        .allow_headers(tower_http::cors::Any);

    let app = Router::new()
        .route(
            "/v1/settings",
            get(api::get_settings).patch(api::patch_settings),
        )
        .route(
            "/v1/accounts",
            get(api::list_accounts).post(api::create_account),
        )
        .route(
            "/v1/accounts/{account_id}",
            get(api::get_account)
                .patch(api::patch_account)
                .delete(api::delete_account),
        )
        .route(
            "/v1/accounts/{account_id}/verify",
            post(api::verify_account),
        )
        .route(
            "/v1/accounts/{account_id}/enable",
            post(api::enable_account),
        )
        .route(
            "/v1/accounts/{account_id}/disable",
            post(api::disable_account),
        )
        .route(
            "/v1/sidebar",
            get(api::get_sidebar),
        )
        .route(
            "/v1/smart-mailboxes",
            get(api::list_smart_mailboxes).post(api::create_smart_mailbox),
        )
        .route(
            "/v1/smart-mailboxes/{smart_mailbox_id}",
            get(api::get_smart_mailbox)
                .patch(api::patch_smart_mailbox)
                .delete(api::delete_smart_mailbox),
        )
        .route(
            "/v1/smart-mailboxes:reset-defaults",
            post(api::reset_default_smart_mailboxes),
        )
        .route(
            "/v1/smart-mailboxes/{smart_mailbox_id}/messages",
            get(api::list_smart_mailbox_messages),
        )
        .route(
            "/v1/views/conversations",
            get(api::list_conversations),
        )
        .route(
            "/v1/views/conversations/{conversation_id}",
            get(api::get_conversation),
        )
        .route(
            "/v1/sources/{source_id}/mailboxes",
            get(api::list_mailboxes),
        )
        .route(
            "/v1/sources/{source_id}/messages",
            get(api::list_source_messages),
        )
        .route(
            "/v1/sources/{source_id}/messages/{message_id}",
            get(api::get_message),
        )
        .route(
            "/v1/sources/{source_id}/commands/messages/{message_id}/set-keywords",
            post(api::set_keywords),
        )
        .route(
            "/v1/sources/{source_id}/commands/messages/{message_id}/add-to-mailbox",
            post(api::add_to_mailbox),
        )
        .route(
            "/v1/sources/{source_id}/commands/messages/{message_id}/remove-from-mailbox",
            post(api::remove_from_mailbox),
        )
        .route(
            "/v1/sources/{source_id}/commands/messages/{message_id}/replace-mailboxes",
            post(api::replace_mailboxes),
        )
        .route(
            "/v1/sources/{source_id}/commands/messages/{message_id}/destroy",
            post(api::destroy_message),
        )
        .route(
            "/v1/sources/{source_id}/commands/sync",
            post(api::trigger_sync),
        )
        .route("/v1/events", get(api::stream_events))
        .layer(cors)
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(&bootstrap.bind_address)
        .await
        .expect("failed to bind daemon listener");
    println!(
        "mail-daemon listening on http://{} (bootstrap: {})",
        bootstrap.bind_address,
        bootstrap.bootstrap_path.display()
    );
    axum::serve(listener, app)
        .await
        .expect("daemon server failed");
}

fn seed_store_if_empty(
    store: &Arc<dyn MailStore>,
    seed: &BootstrapSeedConfig,
) -> Result<(), String> {
    if !store
        .list_accounts()
        .map_err(|err| err.to_string())?
        .is_empty()
    {
        return Ok(());
    }

    store
        .put_app_settings(&seed_app_settings(seed))
        .map_err(|err| err.to_string())?;
    for account in seed_accounts(seed)? {
        store
            .create_account(&account)
            .map_err(|err| err.to_string())?;
    }
    Ok(())
}
