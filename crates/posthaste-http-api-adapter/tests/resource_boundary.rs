//! Anti-divergence guard for the lazy-resource unification.
//!
//! Message body and attachments are served through one path with one transform
//! chokepoint, so they cannot drift into two implementations or grow a second
//! (e.g. unsanitized) body path. These source-scan tests fail if a future change
//! sanitizes the body somewhere other than the single resource transform, or
//! builds a resource byte response outside the shared builder.

use std::fs;
use std::path::{Path, PathBuf};

fn src_files() -> Vec<PathBuf> {
    let mut out = Vec::new();
    collect(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut out);
    out
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("read_dir") {
        let path = entry.expect("entry").path();
        if path.is_dir() {
            collect(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// Non-comment lines as `(1-based line number, text)`.
fn code_lines(path: &Path) -> Vec<(usize, String)> {
    fs::read_to_string(path)
        .expect("read")
        .lines()
        .enumerate()
        .map(|(i, l)| (i + 1, l.to_string()))
        .filter(|(_, l)| !l.trim_start().starts_with("//"))
        .collect()
}

fn project_path(path: &Path) -> String {
    path.strip_prefix(env!("CARGO_MANIFEST_DIR"))
        .unwrap_or(path)
        .display()
        .to_string()
        .replace('\\', "/")
}

/// The body-HTML sanitizer must be invoked from exactly one place — the resource
/// serve transform — so no endpoint can serve body HTML unsanitized.
#[test]
fn body_html_is_sanitized_at_a_single_chokepoint() {
    let mut sites = Vec::new();
    for file in src_files() {
        let rel = project_path(&file);
        // Skip the sanitizer's own module (definition + its unit tests).
        if rel.contains("/sanitize.rs") || rel.contains("/sanitize/") {
            continue;
        }
        for (line_no, line) in code_lines(&file) {
            if line.contains("sanitize_email_html(") {
                sites.push(format!("{rel}:{line_no}"));
            }
        }
    }
    assert_eq!(
        sites.len(),
        1,
        "body-HTML sanitization must have exactly one call site (the resource \
         serve transform); found {sites:?}"
    );
    assert!(
        sites[0].contains("api/messages/detail.rs"),
        "the sole sanitize call must be the resource transform; found {}",
        sites[0]
    );
}

/// Resource byte responses (attachment, body) must be built through the shared
/// `serve_resource_response` builder, never a bespoke per-endpoint response.
#[test]
fn resource_bytes_are_served_through_one_builder() {
    let mut sites = Vec::new();
    for file in src_files() {
        let rel = project_path(&file);
        for (line_no, line) in code_lines(&file) {
            // The builder is invoked once (in serve_message_resource); its own
            // definition line (`fn serve_resource_response`) is excluded.
            if line.contains("serve_resource_response(") && !line.contains("fn ") {
                sites.push(format!("{rel}:{line_no}"));
            }
        }
    }
    assert_eq!(
        sites.len(),
        1,
        "resource bytes must be served through exactly one builder call \
         (serve_message_resource); found {sites:?}"
    );
    assert!(
        sites[0].contains("api/messages/detail.rs"),
        "found {}",
        sites[0]
    );
}
