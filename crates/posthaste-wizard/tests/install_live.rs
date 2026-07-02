//! Live install shakeout: serve a release tarball + SHA256SUMS over a localhost
//! HTTP server and run the real `install` path through it — exercising the
//! actual `ureq` transport, gzip/tar extraction, binary placement, and
//! provisioning end to end. Hermetic (localhost only, no network egress, no
//! systemd), so it is safe in CI.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::sync::mpsc;
use std::thread;

use posthaste_wizard::{
    install, Channel, GithubSource, InstallOptions, Plan, Role, ServiceScope, Version,
};
use sha2::{Digest, Sha256};

/// Build a gzip tarball with `bin/<binary>` holding `contents`, matching the
/// `tools/package/bin.sh` layout the release publishes.
fn make_tarball(dir_name: &str, binary: &str, contents: &[u8]) -> Vec<u8> {
    let mut tar = tar::Builder::new(Vec::new());
    let mut header = tar::Header::new_gnu();
    header.set_size(contents.len() as u64);
    header.set_mode(0o755);
    header.set_cksum();
    tar.append_data(&mut header, format!("{dir_name}/bin/{binary}"), contents)
        .unwrap();
    let tar_bytes = tar.into_inner().unwrap();
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    gz.write_all(&tar_bytes).unwrap();
    gz.finish().unwrap()
}

/// A throwaway localhost HTTP server that serves a fixed map of path -> bytes,
/// then shuts down. Returns the base URL once it is listening.
fn serve(routes: Vec<(String, Vec<u8>)>) -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let base = format!("http://{addr}");
    let (ready_tx, ready_rx) = mpsc::channel();

    let handle = thread::spawn(move || {
        ready_tx.send(()).unwrap();
        // Serve exactly the number of requests the install makes (tarball +
        // SHA256SUMS), then exit so the test thread can join.
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

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn base_plan(role: Role, dir: &Path) -> Plan {
    Plan {
        role,
        config_root: dir.join("config"),
        state_root: dir.join("state"),
        bind: "0.0.0.0:3002".into(),
        tls: false,
        hosts: vec!["authority-server.lan".into()],
        link_serve_token: Some("link-secret".into()),
        link_authority_server_url: None,
        link_token: Some("link-secret".into()),
        exec_path: None,
        systemd_unit_path: None,
    }
}

#[test]
fn install_fetches_verifies_and_provisions_over_http() {
    let dir = tempfile::tempdir().unwrap();
    // Redirect the systemd --user unit dir into the tempdir so the install does
    // not touch the real ~/.config. Set before install computes user_unit_dir().
    std::env::set_var("XDG_CONFIG_HOME", dir.path().join("xdg"));

    // A real gzip tarball carrying a stand-in authority server binary.
    let artifact = "PosthasteAuthorityServerNightly-linux-x86_64";
    let tarball_name = format!("{artifact}.tar.gz");
    let fake_binary = b"#!/bin/sh\necho posthaste-authority-server\n";
    let tarball = make_tarball(artifact, "posthaste-authority-server", fake_binary);
    let sums = format!("{}  {}\n", sha256_hex(&tarball), tarball_name);

    let (base_url, server) = serve(vec![
        (tarball_name, tarball),
        ("SHA256SUMS".to_string(), sums.into_bytes()),
    ]);

    // Real GithubSource (real ureq), just pointed at the localhost server.
    let source = GithubSource::new("theoryzhenkov/posthaste").with_base_url(&base_url);

    let bin_dir = dir.path().join("bin");
    let plan = base_plan(Role::AuthorityServer, dir.path());
    let opts = InstallOptions {
        version: Version::Channel(Channel::Nightly),
        platform: Some("linux-x86_64".into()),
        bin_dir: bin_dir.clone(),
        // Write the user systemd unit (into the tempdir XDG above) so we can
        // assert it references the installed binary; the actual `systemctl`
        // call is absent here and surfaces as a harmless warning.
        service: ServiceScope::UserSystemd,
        enable_linger: false,
    };

    let installed = install(plan, &opts, &source).expect("install over http succeeds");
    server.join().unwrap();

    // Binary landed, executable, with the served bytes.
    let placed = bin_dir.join("posthaste-authority-server");
    assert_eq!(installed.binary_path, placed);
    assert_eq!(std::fs::read(&placed).unwrap(), fake_binary);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&placed).unwrap().permissions().mode();
        assert_eq!(mode & 0o111, 0o111, "installed binary must be executable");
    }

    // Node provisioned: app.toml written, and the unit ExecStart points at the
    // installed binary.
    let app_toml = std::fs::read_to_string(&installed.provisioned.app_toml_path).unwrap();
    assert!(
        app_toml.contains("[link]"),
        "authority_server writes a [link] section"
    );
    let unit_path = installed
        .service_path
        .clone()
        .expect("a user unit path is computed");
    let unit = std::fs::read_to_string(&unit_path).unwrap();
    assert!(
        unit.contains(&placed.display().to_string()),
        "unit ExecStart must reference the installed binary"
    );

    // A authority server node emits a join string for the runtime machine.
    assert!(
        installed.join_string.is_some(),
        "authority_server install emits a join string"
    );
}
