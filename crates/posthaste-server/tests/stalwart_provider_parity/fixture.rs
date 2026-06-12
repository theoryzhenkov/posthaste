use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

use posthaste_domain::{
    AccountTransportSettings, ImapTransportSettings, ProviderAuthKind, ProviderHint, SecretKind,
    SecretRef, SmtpTransportSettings, TransportSecurity,
};

use crate::util::{free_loopback_port, stalwart_bin, temp_root, workspace_root};

pub(super) struct StalwartFixture {
    child: Child,
    root: PathBuf,
    pub(super) http_url: String,
    pub(super) imap_port: u16,
    pub(super) smtp_port: u16,
    pub(super) password: String,
}

impl StalwartFixture {
    pub(super) fn start() -> Self {
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

    pub(super) fn jmap_transport(&self) -> AccountTransportSettings {
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

    pub(super) fn imap_transport(&self) -> AccountTransportSettings {
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

    pub(super) fn email(&self) -> String {
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
