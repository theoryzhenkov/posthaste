use super::*;

#[test]
fn default_filter_keeps_dependencies_quiet() {
    let directives = default_filter_directives("debug");

    assert!(directives.contains("warn,"));
    assert!(directives.contains("posthaste_server=debug"));
    assert!(!directives.contains("imap_next=debug"));
    assert!(!directives.contains("rustls=debug"));
}

#[test]
fn default_filter_sanitizes_invalid_levels() {
    let directives = default_filter_directives("verbose");

    assert!(directives.contains("posthaste_server=info"));
}
