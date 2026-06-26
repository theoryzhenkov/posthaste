use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

use posthaste_domain::{
    AccountTransportSettings, ImapTransportSettings, ProviderAuthKind, ProviderHint, SecretKind,
    SecretRef, SmtpTransportSettings, TransportSecurity,
};

use crate::paths::{free_loopback_port, stalwart_bin, temp_root, workspace_root};

/// A disposable real Stalwart mail server bound to loopback ports.
///
/// Spawns the `stalwart` binary (overridable via `POSTHASTE_STALWART_BIN`)
/// against `tools/dev/stalwart/config.toml`, seeds a `dev@example.org`
/// mailbox via `tools/dev/stalwart/seed.sh`, and tears both down on drop.
///
/// Tests that use this must gate themselves on
/// `POSTHASTE_STALWART_INTEGRATION=1` (the codebase convention) and skip
/// otherwise — the fixture panics if the binary cannot start, since a missing
/// Stalwart is an environment failure, not a test failure.
pub struct StalwartFixture {
    child: Child,
    root: PathBuf,
    pub http_url: String,
    pub imap_port: u16,
    pub smtp_port: u16,
    pub password: String,
}

impl StalwartFixture {
    /// Starts Stalwart on free loopback ports and seeds the dev mailbox.
    pub fn start() -> Self {
        let root = temp_root("posthaste-testkit-stalwart");
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

    /// JMAP transport settings pointing at this fixture's HTTP URL.
    pub fn jmap_transport(&self) -> AccountTransportSettings {
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

    /// IMAP+SMTP transport settings pointing at this fixture's loopback ports.
    pub fn imap_transport(&self) -> AccountTransportSettings {
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

    /// The seeded dev mailbox address.
    pub fn email(&self) -> String {
        "dev@example.org".to_string()
    }

    /// SMTP-deliver `count` messages to the dev mailbox as a self-send (Stalwart
    /// restricts the `dev` user to its own sender address), authenticating as
    /// `dev`, over a single pooled SMTP connection. This is the "message sent"
    /// injection point for live-convergence scenarios: the app's own sync path
    /// (push, with a short poll fallback) observes them.
    pub async fn inject(&self, count: usize) {
        use posthaste_domain::{Recipient, SendMessageRequest};
        use posthaste_imap::{send_smtp_messages, SmtpConnectionConfig};

        let config = SmtpConnectionConfig {
            host: "127.0.0.1".to_string(),
            port: self.smtp_port,
            security: TransportSecurity::Plain,
            sender_name: Some("Injector".to_string()),
            sender_email: self.email(),
            username: "dev".to_string(),
            secret: self.password.clone(),
            auth: ProviderAuthKind::Password,
            provider: ProviderHint::Generic,
        };
        let requests: Vec<SendMessageRequest> = (0..count)
            .map(|i| SendMessageRequest {
                from: Some(Recipient {
                    name: Some("Injector".to_string()),
                    email: self.email(),
                }),
                to: vec![Recipient {
                    name: Some("Dev".to_string()),
                    email: self.email(),
                }],
                cc: Vec::new(),
                bcc: Vec::new(),
                subject: format!("Injected message {i}"),
                body: format!("Injected body {i}"),
                ..Default::default()
            })
            .collect();
        send_smtp_messages(&config, &requests)
            .await
            .unwrap_or_else(|error| panic!("inject batch should deliver: {error:?}"));
    }
}

impl Drop for StalwartFixture {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
