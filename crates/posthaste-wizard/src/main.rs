//! `posthaste-wizard` CLI: provision or install a node for a role, or install
//! the `posthastectl` scripting CLI.
//!
//! `provision` writes config + TLS for a binary you already have. `install` is
//! the one-button path — it fetches the role binary from the release, verifies
//! it, installs it, provisions the node, and registers a `systemd --user`
//! service that keeps it running. `ctl install`/`register`/`status` is the
//! same one-button treatment for `posthastectl` itself (RFC-L2-scripting §7
//! ruling 10b) — see `posthaste_wizard::ctl`.
//!
//! Example — install a TLS authority server node, then a runtime node that joins it:
//!
//! ```text
//! # On the authority server machine:
//! posthaste-wizard install --role authority server --tls --host authority server.lan \
//!   --bind 0.0.0.0:3002 --link-token <secret> \
//!   --config-root ~/.config/mail --state-root ~/.local/share/mail
//! #   ... prints a one-line join string ...
//!
//! # On the runtime machine — one command, no manual URL/token/CA copying:
//! posthaste-wizard install --role runtime --bind 0.0.0.0:3001 \
//!   --config-root ~/.config/mail --state-root ~/.local/share/mail \
//!   --join <join-string-from-authority-server>
//! ```
//!
//! @spec docs/eph/PLAN-L2-install-wizard

use std::path::PathBuf;
use std::process::ExitCode;

use posthaste_wizard::{
    apply_join, guided_install, install, install_ctl, provision, register, Channel,
    CtlInstallOptions, CtlSource, GithubSource, InstallOptions, Plan, Role, ServiceScope, Version,
};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("provision") => run_provision(&args[1..]),
        Some("install") => run_install(&args[1..]),
        Some("ctl") => run_ctl(&args[1..]),
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
    let raw = match RawArgs::parse(args) {
        Ok(raw) => raw,
        Err(e) => return arg_error(&e),
    };
    let plan = match raw.into_plan() {
        Ok(plan) => plan,
        Err(e) => return arg_error(&e),
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

fn run_install(args: &[String]) -> ExitCode {
    // No args, or an explicit -i/--interactive, drops into the guided flow
    // rather than erroring on the missing required flags.
    if args.is_empty() || args.iter().any(|a| a == "-i" || a == "--interactive") {
        return run_guided_install();
    }

    let raw = match RawArgs::parse(args) {
        Ok(raw) => raw,
        Err(e) => return arg_error(&e),
    };

    // Capture install-only options before consuming `raw` into the plan.
    let version = match &raw.version {
        Some(tag) => Version::Pinned(tag.clone()),
        None => Version::Channel(Channel::Nightly),
    };
    let platform = raw.platform.clone();
    let join = raw.join.clone();
    let raw_system = raw.system;
    let raw_no_service = raw.no_service;
    let bin_dir = match raw.bin_dir.clone() {
        Some(dir) => dir,
        None => match default_bin_dir() {
            Ok(dir) => dir,
            Err(e) => return arg_error(&e),
        },
    };

    let plan = match raw.into_plan_for_install() {
        Ok(plan) => plan,
        Err(e) => return arg_error(&e),
    };

    let service = if raw_no_service {
        ServiceScope::None
    } else {
        ServiceScope::detect(raw_system)
    };
    execute_install(plan, version, platform, service, bin_dir, join)
}

/// Run the guided prompt flow, then execute the resulting install.
fn run_guided_install() -> ExitCode {
    let stdin = std::io::stdin();
    let mut input = stdin.lock();
    let mut out = std::io::stdout();
    let guided = match guided_install(&mut input, &mut out) {
        Ok(g) => g,
        Err(e) => {
            eprintln!("\n{e}");
            return ExitCode::from(2);
        }
    };
    execute_install(
        guided.plan,
        guided.version,
        // The guided flow does not ask for a cross-platform override; detect it.
        None,
        guided.service,
        guided.bin_dir,
        guided.join,
    )
}

/// Shared install execution for both the flag and guided paths: apply any join
/// string, then fetch + install + register and report.
fn execute_install(
    mut plan: Plan,
    version: Version,
    platform: Option<String>,
    service: ServiceScope,
    bin_dir: PathBuf,
    join: Option<String>,
) -> ExitCode {
    // A runtime node can wire itself from a join string — set authority server URL +
    // token (+ CA) before provisioning.
    if let Some(join) = &join {
        match apply_join(&mut plan, join) {
            Ok(Some(ca)) => println!("trusting authority server CA: {}", ca.display()),
            Ok(None) => {}
            Err(e) => return arg_error(&e),
        }
    }

    let opts = InstallOptions {
        version,
        platform,
        bin_dir,
        service,
        enable_linger: true,
    };
    let source = GithubSource::posthaste();

    match install(plan, &opts, &source) {
        Ok(out) => {
            println!("installed {}", out.binary_path.display());
            println!("  config:  {}", out.provisioned.app_toml_path.display());
            if let Some(svc) = &out.service_path {
                println!("  service: {}", svc.display());
            }
            for w in &out.warnings {
                eprintln!("warning: {w}");
            }
            if let Some(join) = &out.join_string {
                println!(
                    "\nRun this on the runtime machine to join it to this node:\n\n  \
                     posthaste-wizard install --role runtime \\\n    \
                     --config-root <dir> --state-root <dir> --join {join}\n"
                );
            } else {
                println!("\nclient connection profile:");
                println!("{}", out.provisioned.client_profile_json);
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("install failed: {e}");
            ExitCode::FAILURE
        }
    }
}

/// `ctl install|register|status`: the wizard as the `posthastectl` installer
/// (RFC-L2-scripting §7 ruling 10b).
fn run_ctl(args: &[String]) -> ExitCode {
    match args.first().map(String::as_str) {
        Some("install") => run_ctl_install(&args[1..]),
        Some("register") => run_ctl_register(&args[1..]),
        Some("status") => run_ctl_status(&args[1..]),
        Some(other) => {
            eprintln!("unknown ctl subcommand '{other}'\n\n{}", usage());
            ExitCode::from(2)
        }
        None => {
            eprintln!("usage: posthaste-wizard ctl <install|register|status>\n\n{}", usage());
            ExitCode::from(2)
        }
    }
}

fn run_ctl_install(args: &[String]) -> ExitCode {
    let raw = match CtlArgs::parse(args) {
        Ok(raw) => raw,
        Err(e) => return arg_error(&e),
    };
    let to_dir = match raw.bin_dir.clone() {
        Some(dir) => dir,
        None => match default_bin_dir() {
            Ok(dir) => dir,
            Err(e) => return arg_error(&e),
        },
    };
    let version = match &raw.version {
        Some(tag) => Version::Pinned(tag.clone()),
        None => Version::Channel(Channel::Nightly),
    };
    let opts = CtlInstallOptions {
        from: raw.from.clone(),
        to_dir: to_dir.clone(),
        version,
        platform: raw.platform.clone(),
    };
    let source = GithubSource::posthaste();

    match install_ctl(&opts, &source) {
        Ok(installed) => {
            let source_desc = match installed.source {
                CtlSource::Explicit => "the path you gave (--from)",
                CtlSource::Sidecar => "the desktop app's bundled sidecar",
                CtlSource::Downloaded => "a verified GitHub release download",
            };
            println!(
                "installed {} ({source_desc})",
                installed.binary_path.display()
            );
            for w in &installed.warnings {
                eprintln!("warning: {w}");
            }
            // ctl register: runs automatically, informational — a fresh
            // install commonly has no app running yet, so this never fails
            // the install itself.
            let report = register(&to_dir);
            print!("\n{}", report.format());
            if !report.all_ok() {
                println!(
                    "\nrun `posthaste-wizard ctl status` again once the app is running to \
                     re-check."
                );
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("ctl install failed: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run_ctl_register(args: &[String]) -> ExitCode {
    run_ctl_checks(args)
}

fn run_ctl_status(args: &[String]) -> ExitCode {
    run_ctl_checks(args)
}

/// Shared by `ctl register` and `ctl status`: both run the identical
/// binary/PATH/app/discovery/probe sequence and are re-runnable; `register`
/// is just the name used right after an install.
fn run_ctl_checks(args: &[String]) -> ExitCode {
    let raw = match CtlArgs::parse(args) {
        Ok(raw) => raw,
        Err(e) => return arg_error(&e),
    };
    let bin_dir = match raw.bin_dir.clone() {
        Some(dir) => dir,
        None => match default_bin_dir() {
            Ok(dir) => dir,
            Err(e) => return arg_error(&e),
        },
    };
    let report = register(&bin_dir);
    print!("{}", report.format());
    if report.all_ok() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// Flags shared by the `ctl` subcommands.
struct CtlArgs {
    from: Option<PathBuf>,
    bin_dir: Option<PathBuf>,
    version: Option<String>,
    platform: Option<String>,
}

impl CtlArgs {
    fn parse(args: &[String]) -> Result<CtlArgs, String> {
        let mut raw = CtlArgs {
            from: None,
            bin_dir: None,
            version: None,
            platform: None,
        };
        let mut i = 0;
        while i < args.len() {
            let flag = args[i].as_str();
            let mut value = || -> Result<String, String> {
                i += 1;
                args.get(i)
                    .cloned()
                    .ok_or_else(|| format!("{flag} requires a value"))
            };
            match flag {
                "--from" => raw.from = Some(PathBuf::from(value()?)),
                "--to" => raw.bin_dir = Some(PathBuf::from(value()?)),
                "--version" => raw.version = Some(value()?),
                "--platform" => raw.platform = Some(value()?),
                other => return Err(format!("unknown flag '{other}'")),
            }
            i += 1;
        }
        Ok(raw)
    }
}

fn arg_error(msg: &str) -> ExitCode {
    eprintln!("error: {msg}\n\n{}", usage());
    ExitCode::from(2)
}

/// `~/.local/bin`, the XDG user binary dir.
fn default_bin_dir() -> Result<PathBuf, String> {
    std::env::var_os("HOME")
        .map(|h| PathBuf::from(h).join(".local").join("bin"))
        .ok_or_else(|| "HOME is not set; pass --bin-dir explicitly".into())
}

/// All flags either command accepts, parsed once. Each command then projects
/// this into the fields it needs (and rejects the ones it doesn't).
struct RawArgs {
    role: Option<Role>,
    config_root: Option<PathBuf>,
    state_root: Option<PathBuf>,
    bind: String,
    tls: bool,
    hosts: Vec<String>,
    link_serve_token: Option<String>,
    link_authority_server_url: Option<String>,
    link_token: Option<String>,
    exec_path: Option<PathBuf>,
    systemd_unit_path: Option<PathBuf>,
    // install-only
    version: Option<String>,
    platform: Option<String>,
    join: Option<String>,
    bin_dir: Option<PathBuf>,
    system: bool,
    no_service: bool,
}

impl RawArgs {
    fn parse(args: &[String]) -> Result<RawArgs, String> {
        let mut raw = RawArgs {
            role: None,
            config_root: None,
            state_root: None,
            bind: String::from("127.0.0.1:3001"),
            tls: false,
            hosts: Vec::new(),
            link_serve_token: None,
            link_authority_server_url: None,
            link_token: None,
            exec_path: None,
            systemd_unit_path: None,
            version: None,
            platform: None,
            join: None,
            bin_dir: None,
            system: false,
            no_service: false,
        };

        let mut i = 0;
        while i < args.len() {
            let flag = args[i].as_str();
            let mut value = || -> Result<String, String> {
                i += 1;
                args.get(i)
                    .cloned()
                    .ok_or_else(|| format!("{flag} requires a value"))
            };
            match flag {
                "--role" => raw.role = Some(Role::parse(&value()?)?),
                "--config-root" => raw.config_root = Some(PathBuf::from(value()?)),
                "--state-root" => raw.state_root = Some(PathBuf::from(value()?)),
                "--bind" => raw.bind = value()?,
                "--tls" => raw.tls = true,
                "--host" => raw.hosts.push(value()?),
                "--link-token" => {
                    // The link secret serves double duty: the authority server role
                    // *serves* it, the runtime role *presents* it.
                    let v = value()?;
                    raw.link_serve_token = Some(v.clone());
                    raw.link_token = Some(v);
                }
                "--link-authority-server-url" => raw.link_authority_server_url = Some(value()?),
                "--exec" => raw.exec_path = Some(PathBuf::from(value()?)),
                "--systemd" => raw.systemd_unit_path = Some(PathBuf::from(value()?)),
                "--version" => raw.version = Some(value()?),
                "--platform" => raw.platform = Some(value()?),
                "--join" => raw.join = Some(value()?),
                "--bin-dir" => raw.bin_dir = Some(PathBuf::from(value()?)),
                "--system" => raw.system = true,
                "--no-service" => raw.no_service = true,
                other => return Err(format!("unknown flag '{other}'")),
            }
            i += 1;
        }
        Ok(raw)
    }

    fn require_common(&self) -> Result<(Role, PathBuf, PathBuf), String> {
        let role = self.role.ok_or("--role is required")?;
        let config_root = self
            .config_root
            .clone()
            .ok_or("--config-root is required")?;
        let state_root = self.state_root.clone().ok_or("--state-root is required")?;
        Ok((role, config_root, state_root))
    }

    fn into_plan(self) -> Result<Plan, String> {
        let (role, config_root, state_root) = self.require_common()?;
        Ok(Plan {
            role,
            config_root,
            state_root,
            bind: self.bind,
            tls: self.tls,
            hosts: self.hosts,
            link_serve_token: self.link_serve_token,
            link_authority_server_url: self.link_authority_server_url,
            link_token: self.link_token,
            exec_path: self.exec_path,
            systemd_unit_path: self.systemd_unit_path,
        })
    }

    /// Like [`into_plan`], but for `install`: `--exec`/`--systemd` are filled in
    /// by the installer, so reject them here to avoid silent confusion.
    fn into_plan_for_install(self) -> Result<Plan, String> {
        if self.exec_path.is_some() {
            return Err("--exec is not used with `install` (the binary path is the install dir + role); use `provision` for an existing binary".into());
        }
        if self.systemd_unit_path.is_some() {
            return Err("--systemd is not used with `install` (the unit is registered under systemctl --user automatically)".into());
        }
        self.into_plan()
    }
}

fn usage() -> &'static str {
    "usage:\n\
     \x20 posthaste-wizard install --role <daemon|authority-server|runtime> \\\n\
     \x20   --config-root <dir> --state-root <dir> [--bind <addr>] [--tls]\n\
     \x20   [--host <name>]... [--link-token <secret>] [--link-authority-server-url <url>]\n\
     \x20   [--version <tag>] [--platform <p>] [--join <string>] [--bin-dir <dir>] [--system] [--no-service] [-i]\n\
     \n\
     \x20 posthaste-wizard provision --role <daemon|authority-server|runtime> \\\n\
     \x20   --config-root <dir> --state-root <dir> [--bind <addr>] [--tls]\n\
     \x20   [--host <name>]... [--link-authority-server-url <url>] [--link-token <secret>]\n\
     \x20   [--exec <binary-path>] [--systemd <unit-path>]\n\
     \n\
     \x20 posthaste-wizard ctl install [--from <path>] [--to <dir>] [--version <tag>] [--platform <p>]\n\
     \x20 posthaste-wizard ctl register [--to <dir>]\n\
     \x20 posthaste-wizard ctl status [--to <dir>]\n\
     \n\
     install fetches + verifies the role binary from the release, installs it to\n\
     ~/.local/bin, provisions the node, and registers a service: systemctl --user on Linux, launchd on macOS (--system for a root systemd unit, --no-service to skip).\n\
     install with no flags (or -i) runs a guided, interactive setup. provision only writes config + TLS for a binary you already have.\n\
     \n\
     ctl install locates a posthastectl binary — an explicit --from path, the desktop\n\
     app's bundled sidecar, or a checksum-verified GitHub release download — and installs\n\
     it to ~/.local/bin/posthastectl (--to overrides); never sudos, refuses and explains on\n\
     a permission error. It then runs the same checks as `ctl register`/`ctl status`:\n\
     binary placed, PATH resolves it, an app is running, its daemon.json parses, and a\n\
     live authenticated probe succeeds — printed as a \u{2713}/\u{2717} table. `ctl status` re-runs\n\
     that table any time.\n"
}
