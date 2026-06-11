use super::*;

pub(crate) fn populate_changed_paths(criteria: &mut SelectionCriteria) -> LabResult<()> {
    if !criteria.changed {
        return Ok(());
    }
    criteria.changed_paths = detect_changed_paths();
    if criteria.changed_paths.is_empty() {
        return Err(LabError::ChangedSelectionNeedsPaths);
    }
    Ok(())
}

pub(crate) fn detect_changed_paths() -> Vec<String> {
    if let Ok(value) = std::env::var("POSTHASTE_LAB_CHANGED_PATHS") {
        let paths = parse_changed_paths(&value);
        if !paths.is_empty() {
            return paths;
        }
    }

    if let Some(root) = command_output("jj", &["root"]).map(PathBuf::from) {
        if let Some(paths) =
            command_lines_in(&root, "jj", &["diff", "--name-only", "-r", "main..@"])
        {
            return paths;
        }
    }

    if let Some(root) = command_output("git", &["rev-parse", "--show-toplevel"]).map(PathBuf::from)
    {
        return merge_changed_path_sources([
            command_lines_in(&root, "git", &["diff", "--name-only", "origin/main...HEAD"]),
            command_lines_in(&root, "git", &["diff", "--name-only"]),
            command_lines_in(&root, "git", &["diff", "--cached", "--name-only"]),
            command_lines_in(
                &root,
                "git",
                &["ls-files", "--others", "--exclude-standard"],
            ),
        ]);
    }

    Vec::new()
}

pub(crate) fn parse_changed_paths(value: &str) -> Vec<String> {
    value
        .lines()
        .flat_map(|line| line.split(';'))
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .map(normalize_lab_path)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

pub(crate) fn merge_changed_path_sources<const N: usize>(
    sources: [Option<Vec<String>>; N],
) -> Vec<String> {
    sources
        .into_iter()
        .flatten()
        .flatten()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

pub(crate) fn command_lines_in(cwd: &Path, command: &str, args: &[&str]) -> Option<Vec<String>> {
    let output = ProcessCommand::new(command)
        .args(args)
        .current_dir(cwd)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    Some(parse_changed_paths(&text))
}

pub(crate) fn required_value(args: &[String], index: usize, option: &str) -> LabResult<String> {
    args.get(index)
        .filter(|value| !value.starts_with("--"))
        .cloned()
        .ok_or_else(|| LabError::Usage(format!("{option} requires a value")))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UsageKind {
    TopLevel,
    SuiteList,
    Verify,
}

pub(crate) fn usage_kind_for_args(args: &[String]) -> Option<UsageKind> {
    match args.first().map(String::as_str) {
        None => Some(UsageKind::TopLevel),
        Some(arg) if is_help_arg(arg) => Some(UsageKind::TopLevel),
        Some("suite") if args.iter().any(|arg| is_help_arg(arg)) => Some(UsageKind::SuiteList),
        Some("verify") if args.iter().any(|arg| is_help_arg(arg)) => Some(UsageKind::Verify),
        _ => None,
    }
}

pub(crate) fn is_help_arg(arg: &str) -> bool {
    arg == "--help" || arg == "-h"
}

pub(crate) fn print_usage_kind(program: &str, usage_kind: UsageKind) {
    match usage_kind {
        UsageKind::TopLevel => print_usage(program),
        UsageKind::SuiteList => print_suite_usage(program),
        UsageKind::Verify => print_verify_usage(program),
    }
}

pub(crate) fn print_usage(program: &str) {
    println!("Usage:");
    println!("  {program} suite list [--tag TAG] [--target TARGET] [--registry PATH] [--changed] [--json]");
    println!("  {program} verify [SUITE_ID] [--tag TAG] [--target TARGET] [--registry PATH] [--run-root PATH] [--changed]");
}

pub(crate) fn print_suite_usage(program: &str) {
    println!(
        "Usage: {program} suite list [--tag TAG] [--target TARGET] [--registry PATH] [--changed] [--json]"
    );
    println!("Note: --changed reads POSTHASTE_LAB_CHANGED_PATHS when set, otherwise falls back to jj diff main..@ or git diff.");
}

pub(crate) fn print_verify_usage(program: &str) {
    println!("Usage: {program} verify [SUITE_ID] [--tag TAG] [--target TARGET] [--registry PATH] [--run-root PATH] [--changed]");
    println!("Note: --changed reads POSTHASTE_LAB_CHANGED_PATHS when set, otherwise falls back to jj diff main..@ or git diff.");
}
