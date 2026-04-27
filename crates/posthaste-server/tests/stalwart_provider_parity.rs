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
    MailboxId, MailboxSummary, MessageSummary, ProviderAuthKind, ProviderHint, Recipient,
    ReplaceMailboxesCommand, SecretKind, SecretRef, SendMessageRequest, SetKeywordsCommand,
    SmtpTransportSettings, SyncTrigger, TransportSecurity, RFC3339_EPOCH,
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
                full_name: Some("Dev Account".to_string()),
                email_patterns: vec!["dev@example.org".to_string()],
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

    fn email(&self) -> String {
        "dev@example.org".to_string()
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
            sender_name: Some("Dev Account".to_string()),
            sender_email: stalwart.email(),
            username: "dev".to_string(),
            secret: stalwart.password.clone(),
            auth: ProviderAuthKind::Password,
            provider: ProviderHint::Generic,
        },
        Some(harness.store.clone()),
    )
    .await
    .expect("IMAP gateway should connect");

    sync_pair(&harness, &jmap_gateway, &imap_gateway).await;

    let jmap_messages = normalized_messages(&harness, "jmap-stalwart");
    let imap_messages = normalized_messages(&harness, "imap-stalwart");

    assert_eq!(jmap_messages, imap_messages);
    assert!(
        jmap_messages.len() >= 8,
        "fixture should contain enough messages to exercise multiple mailbox roles"
    );

    let initial_imap_location_count = imap_location_count(&harness);
    sync_pair(&harness, &jmap_gateway, &imap_gateway).await;
    assert_eq!(
        jmap_messages,
        normalized_messages(&harness, "jmap-stalwart")
    );
    assert_eq!(
        imap_messages,
        normalized_messages(&harness, "imap-stalwart")
    );
    assert_eq!(initial_imap_location_count, imap_location_count(&harness));

    let target = jmap_message_by_subject(
        &harness,
        "jmap-stalwart",
        "Welcome to the Posthaste sandbox",
    );
    harness
        .service
        .set_keywords(
            &AccountId::from("jmap-stalwart"),
            &target.id,
            &SetKeywordsCommand {
                add: vec!["$flagged".to_string()],
                remove: vec!["$seen".to_string()],
            },
            &jmap_gateway,
        )
        .await
        .expect("JMAP flag mutation should succeed");
    sync_pair(&harness, &jmap_gateway, &imap_gateway).await;

    let jmap_target = message_by_subject(
        &harness,
        "jmap-stalwart",
        "Welcome to the Posthaste sandbox",
    );
    let imap_target = message_by_subject(
        &harness,
        "imap-stalwart",
        "Welcome to the Posthaste sandbox",
    );
    assert!(!jmap_target.is_read);
    assert!(jmap_target.is_flagged);
    assert_eq!(jmap_target.is_read, imap_target.is_read);
    assert_eq!(jmap_target.is_flagged, imap_target.is_flagged);
    assert_eq!(
        normalized_messages(&harness, "jmap-stalwart"),
        normalized_messages(&harness, "imap-stalwart")
    );

    let archive_id = mailbox_id_by_label(&harness, "jmap-stalwart", "archive");
    harness
        .service
        .replace_mailboxes(
            &AccountId::from("jmap-stalwart"),
            &jmap_target.id,
            &ReplaceMailboxesCommand {
                mailbox_ids: vec![archive_id],
            },
            &jmap_gateway,
        )
        .await
        .expect("JMAP mailbox move should succeed");
    sync_pair(&harness, &jmap_gateway, &imap_gateway).await;

    let jmap_labels = mailbox_labels_for_subject(
        &harness,
        "jmap-stalwart",
        "Welcome to the Posthaste sandbox",
    );
    let imap_labels = mailbox_labels_for_subject(
        &harness,
        "imap-stalwart",
        "Welcome to the Posthaste sandbox",
    );
    assert_eq!(jmap_labels, BTreeSet::from(["archive".to_string()]));
    assert_eq!(jmap_labels, imap_labels);
    assert_eq!(
        normalized_messages(&harness, "jmap-stalwart"),
        normalized_messages(&harness, "imap-stalwart")
    );

    let deleted = jmap_message_by_subject(
        &harness,
        "jmap-stalwart",
        "Build failure on obsolete branch",
    );
    harness
        .service
        .destroy_message(
            &AccountId::from("jmap-stalwart"),
            &deleted.id,
            &jmap_gateway,
        )
        .await
        .expect("JMAP delete should succeed");
    sync_pair(&harness, &jmap_gateway, &imap_gateway).await;

    assert!(maybe_message_by_subject(
        &harness,
        "jmap-stalwart",
        "Build failure on obsolete branch"
    )
    .is_none());
    assert!(maybe_message_by_subject(
        &harness,
        "imap-stalwart",
        "Build failure on obsolete branch"
    )
    .is_none());
    assert_eq!(
        normalized_messages(&harness, "jmap-stalwart"),
        normalized_messages(&harness, "imap-stalwart")
    );

    harness
        .service
        .send_message(
            &AccountId::from("jmap-stalwart"),
            &SendMessageRequest {
                to: vec![Recipient {
                    name: Some("Dev Account".to_string()),
                    email: stalwart.email(),
                }],
                cc: Vec::new(),
                bcc: Vec::new(),
                subject: "JMAP parity self-send".to_string(),
                body: "Sent through the JMAP gateway.".to_string(),
                in_reply_to: None,
                references: None,
            },
            &jmap_gateway,
        )
        .await
        .expect("JMAP send should succeed");
    sync_pair(&harness, &jmap_gateway, &imap_gateway).await;

    assert_eq!(
        normalized_messages(&harness, "jmap-stalwart"),
        normalized_messages(&harness, "imap-stalwart")
    );
    let sent_labels =
        mailbox_labels_for_subject(&harness, "imap-stalwart", "JMAP parity self-send");
    assert!(
        sent_labels.contains("sent") || sent_labels.contains("inbox"),
        "self-send should be visible in sent or inbox after sync"
    );

    harness
        .service
        .send_message(
            &AccountId::from("imap-stalwart"),
            &SendMessageRequest {
                to: vec![Recipient {
                    name: Some("Dev Account".to_string()),
                    email: stalwart.email(),
                }],
                cc: Vec::new(),
                bcc: Vec::new(),
                subject: "SMTP parity self-send".to_string(),
                body: "Sent through the IMAP/SMTP gateway.".to_string(),
                in_reply_to: None,
                references: None,
            },
            &imap_gateway,
        )
        .await
        .expect("SMTP send should succeed");
    sync_pair(&harness, &jmap_gateway, &imap_gateway).await;

    assert_eq!(
        normalized_messages(&harness, "jmap-stalwart"),
        normalized_messages(&harness, "imap-stalwart")
    );
    assert!(
        maybe_message_by_subject(&harness, "jmap-stalwart", "SMTP parity self-send").is_some(),
        "SMTP self-send should be visible through JMAP after sync"
    );
    let smtp_sent_labels =
        mailbox_labels_for_subject(&harness, "imap-stalwart", "SMTP parity self-send");
    assert!(
        smtp_sent_labels.contains("sent") || smtp_sent_labels.contains("inbox"),
        "SMTP self-send should be visible in sent or inbox after sync"
    );
}

async fn sync_pair(
    harness: &Harness,
    jmap_gateway: &LiveJmapGateway,
    imap_gateway: &LiveImapSmtpGateway,
) {
    harness
        .service
        .sync_account(
            &AccountId::from("jmap-stalwart"),
            SyncTrigger::Manual,
            jmap_gateway,
        )
        .await
        .expect("JMAP sync should succeed");
    harness
        .service
        .sync_account(
            &AccountId::from("imap-stalwart"),
            SyncTrigger::Manual,
            imap_gateway,
        )
        .await
        .expect("IMAP sync should succeed");
}

fn normalized_messages(harness: &Harness, account_id: &str) -> Vec<String> {
    let mut messages = harness
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
        .collect::<Vec<_>>();
    messages.sort();
    messages
}

fn jmap_message_by_subject(harness: &Harness, account_id: &str, subject: &str) -> MessageSummary {
    message_by_subject(harness, account_id, subject)
}

fn message_by_subject(harness: &Harness, account_id: &str, subject: &str) -> MessageSummary {
    maybe_message_by_subject(harness, account_id, subject)
        .unwrap_or_else(|| panic!("message with subject {subject:?} should exist"))
}

fn maybe_message_by_subject(
    harness: &Harness,
    account_id: &str,
    subject: &str,
) -> Option<MessageSummary> {
    harness
        .service
        .list_messages(&AccountId::from(account_id), None)
        .expect("messages should list")
        .into_iter()
        .find(|message| message.subject.as_deref() == Some(subject))
}

fn mailbox_id_by_label(harness: &Harness, account_id: &str, label: &str) -> MailboxId {
    harness
        .service
        .list_mailboxes(&AccountId::from(account_id))
        .expect("mailboxes should list")
        .into_iter()
        .find(|mailbox| mailbox_label(mailbox) == label)
        .map(|mailbox| mailbox.id)
        .unwrap_or_else(|| panic!("mailbox with label {label:?} should exist"))
}

fn mailbox_labels_for_subject(
    harness: &Harness,
    account_id: &str,
    subject: &str,
) -> BTreeSet<String> {
    let mailboxes = harness
        .service
        .list_mailboxes(&AccountId::from(account_id))
        .expect("mailboxes should list")
        .into_iter()
        .map(|mailbox| (mailbox.id.clone(), mailbox_label(&mailbox)))
        .collect::<std::collections::BTreeMap<_, _>>();
    message_by_subject(harness, account_id, subject)
        .mailbox_ids
        .into_iter()
        .map(|mailbox_id| {
            mailboxes
                .get(&mailbox_id)
                .cloned()
                .unwrap_or_else(|| mailbox_id.to_string())
        })
        .collect()
}

fn mailbox_label(mailbox: &MailboxSummary) -> String {
    mailbox
        .role
        .clone()
        .unwrap_or_else(|| mailbox.name.to_ascii_lowercase())
}

fn imap_location_count(harness: &Harness) -> usize {
    use posthaste_domain::ImapMessageLocationStore;

    harness
        .service
        .list_messages(&AccountId::from("imap-stalwart"), None)
        .expect("IMAP messages should list")
        .into_iter()
        .map(|message| {
            harness
                .store
                .list_imap_message_locations(&AccountId::from("imap-stalwart"), &message.id)
                .expect("IMAP locations should list")
                .len()
        })
        .sum()
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
