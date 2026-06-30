//! Round-trip proof: a node provisioned by the wizard is read back cleanly by
//! the daemon's own config schema (`posthaste-config`). This is the contract
//! that keeps the wizard's rendered TOML keys in lock-step with the schema — if
//! a field is renamed on either side, this test fails.

use std::path::PathBuf;

use posthaste_config::TomlConfigRepository;
use posthaste_wizard::{provision, Plan, Role};

fn base_plan(role: Role, dir: &std::path::Path) -> Plan {
    Plan {
        role,
        config_root: dir.join("config"),
        state_root: dir.join("state"),
        bind: "0.0.0.0:3001".into(),
        tls: false,
        hosts: Vec::new(),
        link_serve_token: None,
        link_backend_url: None,
        link_token: None,
        exec_path: None,
        systemd_unit_path: None,
    }
}

#[test]
fn tls_runtime_node_roundtrips_through_the_daemon_schema() {
    let tmp = tempfile::tempdir().unwrap();
    let mut plan = base_plan(Role::Runtime, tmp.path());
    plan.tls = true;
    plan.hosts = vec!["mail.lan".into(), "127.0.0.1".into()];
    plan.link_backend_url = Some("https://backend.lan:3002/v1".into());
    plan.link_token = Some("shared-secret".into());

    let out = provision(&plan).expect("provision");

    // The daemon's schema reads the wizard's output without error...
    let repo = TomlConfigRepository::open(&plan.config_root).expect("open config");
    let app = repo.read_app_toml().expect("read app.toml");

    // ...and the load-bearing fields survive the round-trip.
    assert_eq!(app.daemon.bind.as_deref(), Some("0.0.0.0:3001"));
    assert_eq!(app.daemon.require_auth, Some(true));
    assert_eq!(app.daemon.allowed_hosts, vec!["mail.lan", "127.0.0.1"]);

    let tls = app.tls.expect("[tls] present");
    assert_eq!(tls.cert, Some(out.leaf_cert_path.unwrap()));
    assert!(tls.key.is_some());

    assert_eq!(
        app.link.backend_url.as_deref(),
        Some("https://backend.lan:3002/v1")
    );
    assert_eq!(app.link.token.as_deref(), Some("shared-secret"));
    assert_eq!(app.link.serve, None); // runtime role does not serve the link

    // The emitted CA exists on disk for the client to trust.
    assert!(out.ca_cert_path.unwrap().exists());
}

#[test]
fn backend_node_serves_the_link() {
    let tmp = tempfile::tempdir().unwrap();
    let mut plan = base_plan(Role::Backend, tmp.path());
    plan.bind = "0.0.0.0:3002".into();
    plan.link_serve_token = Some("shared-secret".into());

    provision(&plan).expect("provision");
    let repo = TomlConfigRepository::open(&plan.config_root).expect("open config");
    let app = repo.read_app_toml().expect("read app.toml");

    assert_eq!(app.link.serve, Some(true));
    assert_eq!(app.link.token.as_deref(), Some("shared-secret"));
    assert!(app.tls.is_none()); // no --tls in this plan
}

#[test]
fn daemon_node_has_no_link_section() {
    let tmp = tempfile::tempdir().unwrap();
    let plan = base_plan(Role::Daemon, tmp.path());

    provision(&plan).expect("provision");
    let repo = TomlConfigRepository::open(&plan.config_root).expect("open config");
    let app = repo.read_app_toml().expect("read app.toml");

    assert_eq!(app.link.serve, None);
    assert_eq!(app.link.backend_url, None);
}

#[test]
fn systemd_unit_references_the_exec_and_roots() {
    let tmp = tempfile::tempdir().unwrap();
    let mut plan = base_plan(Role::Daemon, tmp.path());
    plan.exec_path = Some(PathBuf::from("/usr/local/bin/posthaste_daemon"));
    plan.systemd_unit_path = Some(tmp.path().join("posthaste.service"));

    let out = provision(&plan).expect("provision");
    let unit_path = out.systemd_unit_path.expect("unit written");
    let unit = std::fs::read_to_string(&unit_path).unwrap();
    // The all-in-one daemon needs the `serve` subcommand in ExecStart.
    assert!(unit.contains("ExecStart=/usr/local/bin/posthaste_daemon serve"));
    assert!(unit.contains("POSTHASTE_CONFIG_ROOT="));
    assert!(unit.contains("WantedBy=default.target"));
}
