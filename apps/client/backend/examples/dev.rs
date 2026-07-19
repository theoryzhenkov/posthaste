//! Dev loop: a self-contained backend over a disposable, seeded Stalwart.
//!
//! Spawns a throwaway Stalwart (config + mail state live in a temp dir, torn
//! down on exit), assembles the backend over temp roots, provisions one JMAP
//! account against it, seeds extra messages, and serves the API on a fixed
//! port with a fixed dev token — matched by the frontend's vite proxy:
//!
//! ```text
//! cargo run -p posthaste-client-backend --example dev
//! POSTHASTE_DEV_TOKEN=dev bun run client:dev     # then open the vite URL
//! ```
//!
//! Everything is ephemeral; stop with Ctrl-C.

use std::sync::Arc;
use std::time::Duration;

use posthaste_client_backend::{serve, AppPaths, AppState, BuildOptions};
use posthaste_domain_model::{AccountDriver, AccountId, AccountSettings};
use posthaste_testkit::{StalwartFixture, TestSecretStore};

const DEV_PORT: u16 = 7365;
const DEV_TOKEN: &str = "dev";
const SEED_MESSAGES: usize = 20;

#[tokio::main]
async fn main() {
    let stalwart = tokio::task::spawn_blocking(StalwartFixture::start)
        .await
        .expect("fixture task");
    println!("stalwart up: jmap on {}", stalwart.http_url);

    let dir = tempfile::tempdir().expect("create temp dir");
    let paths = AppPaths::with_roots(dir.path().join("config"), dir.path().join("state"));
    let mut options = BuildOptions::at(paths);
    options.poll_interval = Duration::from_secs(5);
    options.secret_store = Some(Arc::new(TestSecretStore::default()));
    let state = AppState::assemble(options).await.expect("assemble backend");

    let transport = stalwart.jmap_transport();
    let secret_ref = transport
        .secret_ref
        .clone()
        .expect("fixture transport carries a secret reference");
    state
        .secret_store
        .save(&secret_ref, &stalwart.password)
        .expect("store the account password");
    let now = time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .expect("format timestamp");
    let settings = AccountSettings {
        id: AccountId::from("dev"),
        name: "dev".to_string(),
        full_name: Some("Dev Account".to_string()),
        signature: None,
        email_patterns: vec![stalwart.email()],
        driver: AccountDriver::Jmap,
        enabled: true,
        appearance: None,
        transport,
        created_at: now.clone(),
        updated_at: now,
    };
    state
        .service
        .insert_source(&settings)
        .expect("insert account settings");
    state.supervisor.start_account(&settings).await;

    // Optional slow mock account for exercising the sync-status UI: set
    // POSTHASTE_DEV_MOCK_SYNC_DELAY_MS to add a second, mock-driver account
    // whose sync cycles sleep for that long, so `Syncing` (and its steps)
    // stay observable in the frontend.
    if let Ok(delay) = std::env::var("POSTHASTE_DEV_MOCK_SYNC_DELAY_MS") {
        if let Ok(millis) = delay.parse::<usize>() {
            posthaste_engine::MockJmapGateway::set_sync_delay_for_tests(millis);
            let now = time::OffsetDateTime::now_utc()
                .format(&time::format_description::well_known::Rfc3339)
                .expect("format timestamp");
            let mock = AccountSettings {
                id: AccountId::from("mockdev"),
                name: "mockdev".to_string(),
                full_name: Some("Slow Mock".to_string()),
                signature: None,
                email_patterns: Vec::new(),
                driver: AccountDriver::Mock,
                enabled: true,
                appearance: None,
                transport: Default::default(),
                created_at: now.clone(),
                updated_at: now,
            };
            state
                .service
                .insert_source(&mock)
                .expect("insert mock account settings");
            state.supervisor.start_account(&mock).await;
            println!("mock account 'mockdev' up with {millis}ms sync delay");
        }
    }
    state
        .supervisor
        .sync_account(&settings.id)
        .await
        .expect("initial sync");

    // Stalwart caps messages per SMTP session and rate-limits sessions:
    // deliver in batches of 10 with a pause, and treat seeding as
    // best-effort — a tripped limiter should not kill the dev loop.
    let stalwart = Arc::new(stalwart);
    let mut seeded = 0;
    while seeded < SEED_MESSAGES {
        let batch = (SEED_MESSAGES - seeded).min(10);
        let fixture = Arc::clone(&stalwart);
        match tokio::spawn(async move { fixture.inject(batch).await }).await {
            Ok(()) => seeded += batch,
            Err(_) => {
                println!("seeding stopped early at {seeded} (rate limit)");
                break;
            }
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    println!("seeded {seeded} extra messages (arriving via push/poll)");

    let server = serve(state, DEV_PORT, DEV_TOKEN.to_string())
        .await
        .expect("bind dev port");
    println!("backend on http://{} (token: {DEV_TOKEN})", server.addr);
    println!("frontend: POSTHASTE_DEV_TOKEN={DEV_TOKEN} bun run client:dev");

    tokio::signal::ctrl_c().await.expect("ctrl-c");
    println!("shutting down");
}
