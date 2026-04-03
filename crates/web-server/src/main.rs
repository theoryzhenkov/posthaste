mod api;
mod config;
mod push;
mod sanitize;
mod secret;
mod supervisor;

use std::sync::Arc;
use std::time::Duration;

use axum::routing::{get, post};
use axum::Router;
use dotenvy::dotenv;
use mail_config::TomlConfigRepository;
use mail_domain::{ConfigRepository, DomainEvent, MailService, MailStore, SecretStore};
use mail_store::DatabaseStore;
use tokio::sync::broadcast;
use tower_http::cors::{AllowOrigin, CorsLayer};

use crate::config::resolve_roots;
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

    let roots = resolve_roots();

    // Open config repository
    let config_repo =
        TomlConfigRepository::open(&roots.config_root).expect("failed to open config directory");

    // Startup flow: initialize config if empty
    if config_repo.is_empty() {
        if let Some(bootstrap_path) = &roots.bootstrap_path {
            config::import_bootstrap(bootstrap_path, &config_repo)
                .expect("failed to import bootstrap template");
            println!(
                "imported bootstrap template from {}",
                bootstrap_path.display()
            );
        } else {
            config_repo
                .initialize_defaults()
                .expect("failed to initialize default config");
            println!("initialized default config");
        }
    }

    // Read daemon settings from config
    let daemon =
        config::read_daemon_settings(&config_repo).expect("failed to read daemon settings");

    // Open SQLite store (state only)
    let db_path = roots.state_root.join("mail.sqlite");
    let store: Arc<dyn MailStore> = Arc::new(
        DatabaseStore::open(&db_path, &roots.state_root).expect("failed to initialize store"),
    );

    // Build service with config
    let config: Arc<dyn ConfigRepository> = Arc::new(config_repo);
    let service = Arc::new(MailService::new(store.clone(), config.clone()));

    // Sync source projections into SQLite
    service
        .sync_source_projections()
        .expect("failed to sync source projections");

    let (event_sender, _) = broadcast::channel(512);
    let secret_store: Arc<dyn SecretStore> = Arc::new(SystemSecretStore);
    let supervisor = Arc::new(AccountSupervisor::new(
        service.clone(),
        store.clone(),
        secret_store.clone(),
        event_sender.clone(),
        Duration::from_secs(daemon.poll_interval_seconds),
    ));

    // Start runtimes from config sources
    for source in service
        .list_sources()
        .expect("failed to load source configuration")
    {
        supervisor.start_account(&source).await;
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
            daemon.cors_origin.parse().expect("invalid CORS origin"),
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
        .route("/v1/sidebar", get(api::get_sidebar))
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
            "/v1/smart-mailboxes/{smart_mailbox_id}/conversations",
            get(api::list_smart_mailbox_conversations),
        )
        .route("/v1/views/conversations", get(api::list_conversations))
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
        .route("/v1/config:reload", post(api::reload_config))
        .route("/v1/events", get(api::stream_events))
        .layer(cors)
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(&daemon.bind_address)
        .await
        .expect("failed to bind daemon listener");
    println!(
        "mail-daemon listening on http://{} (config: {})",
        daemon.bind_address,
        roots.config_root.display()
    );
    axum::serve(listener, app)
        .await
        .expect("daemon server failed");
}
