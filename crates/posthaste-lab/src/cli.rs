use posthaste_config::validate_config_root;

use super::*;

pub fn default_registry_path() -> PathBuf {
    PathBuf::from("tools/lab/suites.toml")
}

pub fn default_run_root() -> PathBuf {
    PathBuf::from("target/lab/runs")
}

pub fn default_config_dir() -> PathBuf {
    std::env::var_os("POSTHASTE_CONFIG_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("var/dev/posthaste/config"))
}

pub fn run_cli(argv: Vec<String>) -> LabResult<()> {
    let program = argv
        .first()
        .cloned()
        .unwrap_or_else(|| "posthaste-lab".to_string());
    let args = argv.iter().skip(1).cloned().collect::<Vec<_>>();
    if let Some(usage_kind) = usage_kind_for_args(&args) {
        print_usage_kind(&program, usage_kind);
        return Ok(());
    }

    match args.first().map(String::as_str) {
        Some("suite") => run_suite_command(&program, &args[1..]),
        Some("verify") => run_verify_command(argv, &args[1..]),
        Some("config") => run_config_command(&program, &args[1..]),
        Some(other) => Err(LabError::Usage(format!(
            "unknown command {other:?}; expected 'suite', 'verify', or 'config'"
        ))),
        None => {
            print_usage(&program);
            Ok(())
        }
    }
}

pub(crate) fn run_suite_command(program: &str, args: &[String]) -> LabResult<()> {
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        print_suite_usage(program);
        return Ok(());
    }
    match args.first().map(String::as_str) {
        Some("list") => {
            let options = parse_list_options(&args[1..])?;
            let registry = SuiteRegistry::load(&options.registry_path)?;
            let mut criteria = SelectionCriteria {
                tags: options.tags,
                targets: options.targets,
                changed: options.changed,
                ..SelectionCriteria::default()
            };
            populate_changed_paths(&mut criteria)?;
            let selected = registry.select(&criteria)?;
            if options.json {
                let output = SuiteListOutput {
                    schema_version: 1,
                    selection: SelectionRecord::from_criteria(&criteria),
                    suites: selected,
                };
                println!("{}", serde_json::to_string_pretty(&output)?);
            } else {
                for suite in selected {
                    println!("{}", suite.id);
                }
            }
            Ok(())
        }
        Some(other) => Err(LabError::Usage(format!(
            "unknown suite command {other:?}; expected 'list'"
        ))),
        None => Err(LabError::Usage(
            "missing suite command; expected 'list'".to_string(),
        )),
    }
}

pub(crate) fn run_verify_command(argv: Vec<String>, args: &[String]) -> LabResult<()> {
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        print_verify_usage(argv.first().map(String::as_str).unwrap_or("posthaste-lab"));
        return Ok(());
    }
    let options = parse_verify_options(args, argv)?;
    let registry = SuiteRegistry::load(&options.registry_path)?;
    let output = write_verify_run(&registry, options)?;
    println!("Lab run: {}", output.run_dir.display());
    println!("Manifest: {}", output.manifest_path.display());
    println!("Summary: {}", output.summary_path.display());
    println!("Selected suites: {}", output.selected_suite_count);
    println!("Status: {:?}", output.status);
    match output.status {
        LabStatus::Passed => Ok(()),
        LabStatus::Blocked => Err(LabError::VerificationBlocked {
            summary_path: output.summary_path.display().to_string(),
        }),
        LabStatus::Skipped => Err(LabError::VerificationSkipped {
            summary_path: output.summary_path.display().to_string(),
        }),
        LabStatus::Failed => Err(LabError::VerificationFailed {
            summary_path: output.summary_path.display().to_string(),
        }),
    }
}

pub(crate) fn run_config_command(program: &str, args: &[String]) -> LabResult<()> {
    match args.first().map(String::as_str) {
        Some("validate") => run_config_validate_command(program, &args[1..]),
        Some(other) => Err(LabError::Usage(format!(
            "unknown config command {other:?}; expected 'validate'"
        ))),
        None => Err(LabError::Usage(
            "missing config command; expected 'validate'".to_string(),
        )),
    }
}

pub(crate) fn run_config_validate_command(program: &str, args: &[String]) -> LabResult<()> {
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        print_config_validate_usage(program);
        return Ok(());
    }
    let options = parse_config_validate_options(args)?;
    if !options.config_dir.is_dir() {
        return Err(LabError::Usage(format!(
            "config dir '{}' does not exist or is not a directory",
            options.config_dir.display()
        )));
    }
    validate_config_root(&options.config_dir).map_err(|error| LabError::ConfigValidation {
        config_dir: options.config_dir.display().to_string(),
        message: error.to_string(),
    })?;
    println!("Config valid: {}", options.config_dir.display());
    Ok(())
}

#[derive(Debug, Clone)]
pub(crate) struct ListOptions {
    pub(crate) registry_path: PathBuf,
    pub(crate) tags: Vec<String>,
    pub(crate) targets: Vec<String>,
    pub(crate) changed: bool,
    pub(crate) json: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct ConfigValidateOptions {
    pub(crate) config_dir: PathBuf,
}

pub(crate) fn parse_list_options(args: &[String]) -> LabResult<ListOptions> {
    let mut registry_path = default_registry_path();
    let mut tags = Vec::new();
    let mut targets = Vec::new();
    let mut changed = false;
    let mut json = false;
    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        match arg.as_str() {
            "--registry" => {
                index += 1;
                registry_path = PathBuf::from(required_value(args, index, "--registry")?);
            }
            "--tag" => {
                index += 1;
                tags.push(required_value(args, index, "--tag")?);
            }
            "--changed" => {
                changed = true;
            }
            "--json" => {
                json = true;
            }
            "--target" => {
                index += 1;
                targets.push(required_value(args, index, "--target")?);
            }
            _ if arg.starts_with("--registry=") => {
                registry_path = PathBuf::from(arg.trim_start_matches("--registry=").to_string());
            }
            _ if arg.starts_with("--tag=") => {
                tags.push(arg.trim_start_matches("--tag=").to_string());
            }
            _ if arg.starts_with("--target=") => {
                targets.push(arg.trim_start_matches("--target=").to_string());
            }
            _ if arg.starts_with("--") => {
                return Err(LabError::Usage(format!("unknown option {arg:?}")));
            }
            _ => {
                return Err(LabError::Usage(format!(
                    "unexpected positional argument {arg:?} for suite list"
                )));
            }
        }
        index += 1;
    }

    Ok(ListOptions {
        registry_path,
        tags,
        targets,
        changed,
        json,
    })
}

pub(crate) fn parse_config_validate_options(args: &[String]) -> LabResult<ConfigValidateOptions> {
    let mut config_dir = default_config_dir();
    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        match arg.as_str() {
            "--config-dir" => {
                index += 1;
                config_dir = PathBuf::from(required_value(args, index, "--config-dir")?);
            }
            _ if arg.starts_with("--config-dir=") => {
                config_dir = PathBuf::from(arg.trim_start_matches("--config-dir=").to_string());
            }
            _ if arg.starts_with("--") => {
                return Err(LabError::Usage(format!("unknown option {arg:?}")));
            }
            _ => {
                return Err(LabError::Usage(format!(
                    "unexpected positional argument {arg:?} for config validate"
                )));
            }
        }
        index += 1;
    }
    Ok(ConfigValidateOptions { config_dir })
}

pub(crate) fn parse_verify_options(args: &[String], argv: Vec<String>) -> LabResult<VerifyOptions> {
    let mut registry_path = default_registry_path();
    let mut run_root = default_run_root();
    let mut criteria = SelectionCriteria::default();
    let mut positional_suite_id = None;
    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        match arg.as_str() {
            "--registry" => {
                index += 1;
                registry_path = PathBuf::from(required_value(args, index, "--registry")?);
            }
            "--run-root" => {
                index += 1;
                run_root = PathBuf::from(required_value(args, index, "--run-root")?);
            }
            "--tag" => {
                index += 1;
                criteria.tags.push(required_value(args, index, "--tag")?);
            }
            "--target" => {
                index += 1;
                criteria
                    .targets
                    .push(required_value(args, index, "--target")?);
            }
            "--changed" => {
                criteria.changed = true;
            }
            _ if arg.starts_with("--registry=") => {
                registry_path = PathBuf::from(arg.trim_start_matches("--registry=").to_string());
            }
            _ if arg.starts_with("--run-root=") => {
                run_root = PathBuf::from(arg.trim_start_matches("--run-root=").to_string());
            }
            _ if arg.starts_with("--tag=") => {
                criteria
                    .tags
                    .push(arg.trim_start_matches("--tag=").to_string());
            }
            _ if arg.starts_with("--target=") => {
                criteria
                    .targets
                    .push(arg.trim_start_matches("--target=").to_string());
            }
            _ if arg.starts_with("--") => {
                return Err(LabError::Usage(format!("unknown option {arg:?}")));
            }
            _ => {
                if positional_suite_id.is_some() {
                    return Err(LabError::Usage(format!(
                        "multiple suite ids supplied; unexpected {arg:?}"
                    )));
                }
                positional_suite_id = Some(arg.clone());
            }
        }
        index += 1;
    }
    criteria.suite_id = positional_suite_id;
    populate_changed_paths(&mut criteria)?;

    Ok(VerifyOptions {
        run_root,
        registry_path,
        argv,
        criteria,
    })
}
