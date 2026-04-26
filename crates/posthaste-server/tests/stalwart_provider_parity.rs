use std::collections::BTreeSet;
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use posthaste_config::TomlConfigRepository;
use posthaste_domain::{
    AccountDriver, AccountId, AccountSettings, AccountTransportSettings, ImapTransportSettings,
    ProviderAuthKind, ProviderHint, SecretKind, SecretRef, SmtpTransportSettings,
    TransportSecurity, RFC3339_EPOCH,
};
use posthaste_engine::LiveJmapGateway;
use posthaste_imap::{ImapConnectionConfig, LiveImapSmtpGateway, SmtpConnectionConfig};
use posthaste_store::DatabaseStore;

static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

struct Harness {
    service: posthaste_domain::MailService,
    store: Arc<DatabaseStore>,
}

impl Harness {
    fn new() -> Self {
        let root = temp_root("posthaste-stalwart-provider-parity");
        let config_root = root.join("config");
        let state_root = root.join("state");
        let config_repo =
            TomlConfigRepository::open(&config_root).expect("config repository should open");
        config_repo
            .initialize_defaults()
            .expect("config defaults should initialize");
        let store = Arc::new(
            DatabaseStore::open(state_root.join("mail.sqlite"), &state_root)
                .expect("database store should open"),
        );
        let config = Arc::new(config_repo);
        Self {
            service: posthaste_domain::MailService::new(store.clone(), config),
            store,
        }
    }

    fn save_account(
        &self,
        id: &str,
        name: &str,
        driver: AccountDriver,
        transport: AccountTransportSettings,
    ) {
        self.service
            .save_source(&AccountSettings {
                id: AccountId::from(id),
                name: name.to_string(),
                full_name: None,
                email_patterns: Vec::new(),
                driver,
                enabled: true,
                appearance: None,
                transport,
                created_at: RFC3339_EPOCH.to_string(),
                updated_at: RFC3339_EPOCH.to_string(),
            })
            .expect("account should save");
    }
}

struct StalwartFixture {
    child: Child,
    root: PathBuf,
    http_url: String,
    imap_port: u16,
    smtp_port: u16,
    password: String,
}

impl StalwartFixture {
    fn start() -> Self {
        let root = temp_root("posthaste-stalwart-server");
        let data = root.join("data");
        let logs = root.join("logs");
        let state = root.join("state");
        std::fs::create_dir_all(&data).expect("data dir");
        std::fs::create_dir_all(&logs).expect("logs dir");
        std::fs::create_dir_all(&state).expect("state dir");

        let http_port = free_loopback_port();
        let imap_port = free_loopback_port();
        let smtp_port = free_loopback_port();
        let http_bind = format!("127.0.0.1:{http_port}");
        let http_url = format!("http://127.0.0.1:{http_port}");
        let imap_bind = format!("127.0.0.1:{imap_port}");
        let smtp_bind = format!("127.0.0.1:{smtp_port}");
        let admin_password = "devadmin";
        let password = "devpass".to_string();
        let workspace_root = workspace_root();
        let config_path = workspace_root.join("tools/dev/stalwart/config.toml");
        let seed_path = workspace_root.join("tools/dev/stalwart/seed.sh");

        let mut child = Command::new(stalwart_bin())
            .arg("-c")
            .arg(config_path)
            .current_dir(&workspace_root)
            .env("POSTHASTE_STALWART_DATA", &data)
            .env("POSTHASTE_STALWART_LOGS", &logs)
            .env("POSTHASTE_STALWART_ADMIN_PASSWORD", admin_password)
            .env("POSTHASTE_STALWART_BIND", &http_bind)
            .env("POSTHASTE_STALWART_URL", &http_url)
            .env("POSTHASTE_STALWART_IMAP_BIND", &imap_bind)
            .env("POSTHASTE_STALWART_SMTP_BIND", &smtp_bind)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("stalwart should start");

        let seed = Command::new("bash")
            .arg(seed_path)
            .current_dir(&workspace_root)
            .env("POSTHASTE_STALWART_URL", &http_url)
            .env("POSTHASTE_STALWART_ADMIN_PASSWORD", admin_password)
            .env("POSTHASTE_STALWART_USER_PASSWORD", &password)
            .env("POSTHASTE_STALWART_DATA", &data)
            .env("POSTHASTE_STATE_ROOT", &state)
            .output()
            .expect("stalwart seed should run");
        if !seed.status.success() {
            let _ = child.kill();
            let _ = child.wait();
            let stdout = String::from_utf8_lossy(&seed.stdout);
            let stderr = String::from_utf8_lossy(&seed.stderr);
            panic!("stalwart seed failed\nstdout:\n{stdout}\nstderr:\n{stderr}");
        }

        Self {
            child,
            root,
            http_url,
            imap_port,
            smtp_port,
            password,
        }
    }

    fn jmap_transport(&self) -> AccountTransportSettings {
        AccountTransportSettings {
            provider: ProviderHint::Generic,
            auth: ProviderAuthKind::Password,
            base_url: Some(self.http_url.clone()),
            username: Some("dev".to_string()),
            secret_ref: Some(SecretRef {
                kind: SecretKind::Env,
                key: "POSTHASTE_UNUSED".to_string(),
            }),
            imap: None,
            smtp: None,
        }
    }

    fn imap_transport(&self) -> AccountTransportSettings {
        AccountTransportSettings {
            provider: ProviderHint::Generic,
            auth: ProviderAuthKind::Password,
            base_url: None,
            username: Some("dev".to_string()),
            secret_ref: Some(SecretRef {
                kind: SecretKind::Env,
                key: "POSTHASTE_UNUSED".to_string(),
            }),
            imap: Some(ImapTransportSettings {
                host: "127.0.0.1".to_string(),
                port: self.imap_port,
                security: TransportSecurity::Plain,
            }),
            smtp: Some(SmtpTransportSettings {
                host: "127.0.0.1".to_string(),
                port: self.smtp_port,
                security: TransportSecurity::Plain,
            }),
        }
    }
}

impl Drop for StalwartFixture {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[tokio::test]
// spec: docs/L0-providers#live-provider-parity
async fn stalwart_jmap_and_imap_sync_project_equivalent_fixture_messages() {
    if std::env::var("POSTHASTE_STALWART_INTEGRATION").as_deref() != Ok("1") {
        eprintln!("skipping Stalwart integration; set POSTHASTE_STALWART_INTEGRATION=1");
        return;
    }

    let stalwart = StalwartFixture::start();
    let harness = Harness::new();
    harness.save_account(
        "jmap-stalwart",
        "Stalwart JMAP",
        AccountDriver::Jmap,
        stalwart.jmap_transport(),
    );
    harness.save_account(
        "imap-stalwart",
        "Stalwart IMAP",
        AccountDriver::ImapSmtp,
        stalwart.imap_transport(),
    );
    let jmap_gateway =
        LiveJmapGateway::connect(&stalwart.http_url, Some("dev"), &stalwart.password)
            .await
            .expect("JMAP gateway should connect");
    let imap_gateway = LiveImapSmtpGateway::connect(
        ImapConnectionConfig {
            host: "127.0.0.1".to_string(),
            port: stalwart.imap_port,
            security: TransportSecurity::Plain,
            username: "dev".to_string(),
            secret: stalwart.password.clone(),
            auth: ProviderAuthKind::Password,
        },
        SmtpConnectionConfig {
            host: "127.0.0.1".to_string(),
            port: stalwart.smtp_port,
            security: TransportSecurity::Plain,
            username: "dev".to_string(),
            secret: stalwart.password.clone(),
            auth: ProviderAuthKind::Password,
            provider: ProviderHint::Generic,
        },
        Some(harness.store.clone()),
    )
    .await
    .expect("IMAP gateway should connect");

    harness
        .service
        .sync_account(
            &AccountId::from("jmap-stalwart"),
            posthaste_domain::SyncTrigger::Manual,
            &jmap_gateway,
        )
        .await
        .expect("JMAP sync should succeed");
    harness
        .service
        .sync_account(
            &AccountId::from("imap-stalwart"),
            posthaste_domain::SyncTrigger::Manual,
            &imap_gateway,
        )
        .await
        .expect("IMAP sync should succeed");

    let jmap_messages = normalized_messages(&harness, "jmap-stalwart");
    let imap_messages = normalized_messages(&harness, "imap-stalwart");

    assert_eq!(jmap_messages, imap_messages);
    assert!(
        jmap_messages.len() >= 8,
        "fixture should contain enough messages to exercise multiple mailbox roles"
    );
}

fn normalized_messages(harness: &Harness, account_id: &str) -> BTreeSet<String> {
    harness
        .service
        .list_messages(&AccountId::from(account_id), None)
        .expect("messages should list")
        .into_iter()
        .map(|message| {
            format!(
                "{}\0{}\0{}\0{}\0{}",
                message.subject.unwrap_or_default(),
                message.from_email.unwrap_or_default(),
                message.has_attachment,
                message.is_read,
                message.is_flagged
            )
        })
        .collect()
}

fn temp_root(prefix: &str) -> PathBuf {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after epoch")
        .as_nanos();
    let seq = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("{prefix}-{now}-{seq}"))
}

fn free_loopback_port() -> u16 {
    TcpListener::bind(("127.0.0.1", 0))
        .expect("free loopback port")
        .local_addr()
        .expect("local addr")
        .port()
}

fn stalwart_bin() -> String {
    std::env::var("POSTHASTE_STALWART_BIN").unwrap_or_else(|_| "stalwart".to_string())
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("workspace root")
        .to_path_buf()
}
