use super::*;

#[test]
fn routes_command_specific_help() {
    let args = |parts: &[&str]| {
        parts
            .iter()
            .map(|part| (*part).to_string())
            .collect::<Vec<_>>()
    };

    assert_eq!(usage_kind_for_args(&args(&[])), Some(UsageKind::TopLevel));
    assert_eq!(
        usage_kind_for_args(&args(&["--help"])),
        Some(UsageKind::TopLevel)
    );
    assert_eq!(
        usage_kind_for_args(&args(&["suite", "list", "--help"])),
        Some(UsageKind::SuiteList)
    );
    assert_eq!(
        usage_kind_for_args(&args(&["verify", "--help"])),
        Some(UsageKind::Verify)
    );
    assert_eq!(usage_kind_for_args(&args(&["suite", "list"])), None);
}
