//! `posthaste-wizard` CLI: provision a node for a role, then print where its
//! config + TLS material landed and the client connection profile to hand out.
//!
//! Example — provision a TLS runtime node over a remote backend:
//!
//! ```text
//! posthaste-wizard provision \
//!   --role runtime --config-root ~/.config/mail --state-root ~/.local/share/mail \
//!   --bind 0.0.0.0:3001 --tls --host mail.lan --host 192.168.1.10 \
//!   --link-backend-url https://backend.lan:3002/v1 --link-token <secret> \
//!   --exec /usr/local/bin/posthaste_runtime_daemon --systemd ./posthaste-runtime.service
//! ```
//!
//! @spec docs/eph/PLAN-L2-install-wizard

use std::path::PathBuf;
use std::process::ExitCode;

use posthaste_wizard::{provision, Plan, Role};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("provision") => run_provision(&args[1..]),
        Some("--help") | Some("-h") | None => {
            print!("{}", usage());
            ExitCode::SUCCESS
        }
        Some(other) => {
            eprintln!("unknown command '{other}'\n\n{}", usage());
            ExitCode::from(2)
        }
    }
}

fn run_provision(args: &[String]) -> ExitCode {
    let plan = match parse_plan(args) {
        Ok(plan) => plan,
        Err(e) => {
            eprintln!("error: {e}\n\n{}", usage());
            return ExitCode::from(2);
        }
    };
    match provision(&plan) {
        Ok(out) => {
            println!("provisioned {} node", plan.role.binary());
            println!("  config:  {}", out.app_toml_path.display());
            if let Some(ca) = &out.ca_cert_path {
                println!("  ca cert: {}", ca.display());
            }
            if let Some(leaf) = &out.leaf_cert_path {
                println!("  tls leaf: {}", leaf.display());
            }
            if let Some(unit) = &out.systemd_unit_path {
                println!("  service: {}", unit.display());
            }
            println!("\nclient connection profile:");
            println!("{}", out.client_profile_json);
            println!(
                "\nNext: start the node, then mint a client token from \
                 <state-root>/daemon.json with `posthaste token attenuate`."
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("provision failed: {e}");
            ExitCode::FAILURE
        }
    }
}

fn parse_plan(args: &[String]) -> Result<Plan, String> {
    let mut role: Option<Role> = None;
    let mut config_root: Option<PathBuf> = None;
    let mut state_root: Option<PathBuf> = None;
    let mut bind = String::from("127.0.0.1:3001");
    let mut tls = false;
    let mut hosts: Vec<String> = Vec::new();
    let mut link_serve_token: Option<String> = None;
    let mut link_backend_url: Option<String> = None;
    let mut link_token: Option<String> = None;
    let mut exec_path: Option<PathBuf> = None;
    let mut systemd_unit_path: Option<PathBuf> = None;

    let mut i = 0;
    while i < args.len() {
        let flag = args[i].as_str();
        // A small helper to read the next arg as this flag's value.
        let mut value = || -> Result<String, String> {
            i += 1;
            args.get(i)
                .cloned()
                .ok_or_else(|| format!("{flag} requires a value"))
        };
        match flag {
            "--role" => role = Some(Role::parse(&value()?)?),
            "--config-root" => config_root = Some(PathBuf::from(value()?)),
            "--state-root" => state_root = Some(PathBuf::from(value()?)),
            "--bind" => bind = value()?,
            "--tls" => tls = true,
            "--host" => hosts.push(value()?),
            "--link-token" => {
                // The link secret serves double duty: the backend role *serves*
                // it, the runtime role *presents* it. Store it in both slots and
                // let the role select which one is rendered.
                let v = value()?;
                link_serve_token = Some(v.clone());
                link_token = Some(v);
            }
            "--link-backend-url" => link_backend_url = Some(value()?),
            "--exec" => exec_path = Some(PathBuf::from(value()?)),
            "--systemd" => systemd_unit_path = Some(PathBuf::from(value()?)),
            other => return Err(format!("unknown flag '{other}'")),
        }
        i += 1;
    }

    let role = role.ok_or("--role is required")?;
    let config_root = config_root.ok_or("--config-root is required")?;
    let state_root = state_root.ok_or("--state-root is required")?;

    Ok(Plan {
        role,
        config_root,
        state_root,
        bind,
        tls,
        hosts,
        link_serve_token,
        link_backend_url,
        link_token,
        exec_path,
        systemd_unit_path,
    })
}

fn usage() -> &'static str {
    "usage: posthaste-wizard provision --role <daemon|backend|runtime> \\\n\
     \x20 --config-root <dir> --state-root <dir> [--bind <addr>] [--tls]\n\
     \x20 [--host <name>]... [--link-backend-url <url>] [--link-token <secret>]\n\
     \x20 [--exec <binary-path>] [--systemd <unit-path>]\n\
     \n\
     Provisions one node's app.toml (+ a local CA/leaf under --tls) and prints\n\
     the client connection profile. One-shot: delete the wizard afterward.\n"
}
