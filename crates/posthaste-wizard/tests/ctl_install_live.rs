//! Live `ctl install` shakeout: serve a `posthastectl` release asset +
//! `SHA256SUMS` over a localhost HTTP server and run the real `install_ctl`
//! path through it (the actual `ureq` transport + checksum verification),
//! then run `register` end to end against a stubbed `daemon.json` and a
//! second localhost mock standing in for the running app. Hermetic
//! (localhost only, no network egress, no systemd), so it is safe in CI.
//!
//! Every test directory here is a `tempfile::TempDir` (P6: RAII-removed on
//! drop, including during a panicking unwind) — the same guard
//! `posthaste-testkit`'s `TempDirGuard` wraps. This crate does not take
//! `posthaste-testkit` as a dev-dependency: it would pull in the full
//! store/engine graph into a crate that is deliberately lean (see
//! `src/lib.rs`'s module doc), and territory for that graph belongs to other
//! in-flight work on this branch.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::sync::mpsc;
use std::thread;

use posthaste_wizard::{install_ctl, register, CtlInstallOptions, CtlSource, GithubSource, Version};
use sha2::{Digest, Sha256};

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// A throwaway localhost HTTP server that serves a fixed map of path -> bytes
/// for exactly `routes.len()` requests, then exits.
fn serve(routes: Vec<(String, Vec<u8>)>) -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let base = format!("http://{addr}");
    let (ready_tx, ready_rx) = mpsc::channel();

    let handle = thread::spawn(move || {
        ready_tx.send(()).unwrap();
        for _ in 0..routes.len() {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0u8; 4096];
            let n = stream.read(&mut buf).unwrap();
            let req = String::from_utf8_lossy(&buf[..n]);
            let path = req
                .lines()
                .next()
                .and_then(|l| l.split_whitespace().nth(1))
                .unwrap_or("")
                .to_string();

            let body = routes
                .iter()
                .find(|(p, _)| path.ends_with(p.as_str()))
                .map(|(_, b)| b.clone());

            match body {
                Some(body) => {
                    let header = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    stream.write_all(header.as_bytes()).unwrap();
                    stream.write_all(&body).unwrap();
                }
                None => {
                    let resp =
                        "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
                    stream.write_all(resp.as_bytes()).unwrap();
                }
            }
        }
    });

    ready_rx.recv().unwrap();
    (base, handle)
}

/// A one-shot localhost server standing in for the running app's `/v1` API:
/// answers a single authenticated GET (the register/status probe).
fn serve_app_probe(expected_token: &str) -> (u16, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let expected = format!("Bearer {expected_token}");
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buf = [0u8; 4096];
        let n = stream.read(&mut buf).unwrap();
        let req = String::from_utf8_lossy(&buf[..n]);
        assert!(req.contains(&expected), "probe must carry the discovery token: {req}");
        let body = b"{}";
        let resp = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        stream.write_all(resp.as_bytes()).unwrap();
        stream.write_all(body).unwrap();
    });
    (port, handle)
}

struct EnvGuard {
    key: &'static str,
    prior: Option<std::ffi::OsString>,
}

impl EnvGuard {
    fn set(key: &'static str, value: impl AsRef<std::ffi::OsStr>) -> Self {
        let prior = std::env::var_os(key);
        std::env::set_var(key, value);
        EnvGuard { key, prior }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        match &self.prior {
            Some(v) => std::env::set_var(self.key, v),
            None => std::env::remove_var(self.key),
        }
    }
}

/// Serializes the whole file: every test mutates process-global env
/// (PATH/HOME/POSTHASTE_*), so they cannot run concurrently.
static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn ctl_install_fetches_and_verifies_a_real_download_over_http() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    // P6: an RAII tempdir, removed on drop (including a panicking unwind) —
    // the exact guarantee `posthaste-testkit::TempDirGuard` wraps `tempfile`
    // with.
    let dir = tempfile::tempdir().unwrap();
    // No sidecar candidate should resolve inside this test's fake HOME/no
    // POSTHASTE_APP_DIR, so `install_ctl` must fall through to the download.
    let _app_dir_guard = EnvGuard::set("POSTHASTE_APP_DIR", dir.path().join("no-such-app-dir"));
    let _home_guard = EnvGuard::set("HOME", dir.path().join("empty-home"));

    let asset_name = "PosthasteCTLNightly-linux-x64";
    let ctl_bytes = b"#!/bin/sh\necho posthastectl\n".to_vec();
    let sums = format!("{}  {}\n", sha256_hex(&ctl_bytes), asset_name);

    let (base_url, server) = serve(vec![
        (asset_name.to_string(), ctl_bytes.clone()),
        ("SHA256SUMS".to_string(), sums.into_bytes()),
    ]);
    let source = GithubSource::new("theoryzhenkov/posthaste").with_base_url(&base_url);

    let to_dir = dir.path().join("bin");
    let opts = CtlInstallOptions {
        from: None,
        to_dir: to_dir.clone(),
        version: Version::Channel(posthaste_wizard::Channel::Nightly),
        platform: Some("linux-x64".into()),
    };

    let installed = install_ctl(&opts, &source).expect("ctl install over http succeeds");
    server.join().unwrap();

    assert_eq!(installed.source, CtlSource::Downloaded);
    assert_eq!(
        installed.binary_path,
        to_dir.join(posthaste_wizard::ctl_binary_name())
    );
    assert_eq!(std::fs::read(&installed.binary_path).unwrap(), ctl_bytes);
    #[cfg(unix)]
    assert_executable(&installed.binary_path);
}

#[test]
fn register_table_finds_the_installed_binary_on_path_and_probes_the_running_app() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();

    // Install into a tempdir bin dir directly (no network needed here — a
    // `--from` install), then point PATH/POSTHASTE_STATE_ROOT at test
    // fixtures and run the same `register` a real `ctl install`/`ctl status`
    // invocation would.
    let source_bin = dir.path().join("fake-ctl-source");
    std::fs::write(&source_bin, b"stand-in ctl").unwrap();
    let to_dir = dir.path().join("bin");
    let opts = CtlInstallOptions {
        from: Some(source_bin),
        to_dir: to_dir.clone(),
        version: Version::Channel(posthaste_wizard::Channel::Nightly),
        platform: None,
    };
    // `--from` is set, so `install_ctl` must not touch the network; a real
    // `GithubSource` (unreachable in this test) proves that.
    let installed = install_ctl(&opts, &GithubSource::posthaste()).expect("from-path install");
    assert_eq!(installed.source, CtlSource::Explicit);

    let token = "bootstrap-mock-token";
    let (port, app_server) = serve_app_probe(token);

    let state_dir = dir.path().join("state");
    std::fs::create_dir_all(&state_dir).unwrap();
    std::fs::write(
        state_dir.join("daemon.json"),
        serde_json::json!({
            "version": 1,
            "port": port,
            "url": format!("http://127.0.0.1:{port}/v1"),
            "token": token,
        })
        .to_string(),
    )
    .unwrap();

    let _state_guard = EnvGuard::set("POSTHASTE_STATE_ROOT", &state_dir);
    let _path_guard = EnvGuard::set("PATH", &to_dir);

    let report = register(&to_dir);
    app_server.join().unwrap();

    assert!(report.binary.ok, "{}", report.binary.detail);
    assert!(report.path.ok, "{}", report.path.detail);
    assert!(report.app_running.ok, "{}", report.app_running.detail);
    assert!(report.discovery.ok, "{}", report.discovery.detail);
    assert!(report.probe.ok, "{}", report.probe.detail);
    assert!(report.all_ok());

    let table = report.format();
    assert!(table.contains('\u{2713}'), "the table shows a pass mark:\n{table}");
    assert!(!table.contains('\u{2717}'), "no failure marks expected:\n{table}");
}

#[cfg(unix)]
fn assert_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let mode = std::fs::metadata(path).unwrap().permissions().mode();
    assert_eq!(mode & 0o111, 0o111, "installed ctl must be executable");
}
