//! Verifies the checked-in `frontend/src/gen/` matches what the models crate
//! generates today. Exports into a temp directory (never the source tree)
//! and diffs; on mismatch the fix is `just gen-ts`.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

/// Reads every generated `.ts` file (including the barrel) as name → content.
fn read_ts_files(dir: &Path) -> BTreeMap<String, String> {
    let mut files = BTreeMap::new();
    for entry in fs::read_dir(dir).expect("read generated dir") {
        let path = entry.expect("dir entry").path();
        if path.extension().is_some_and(|ext| ext == "ts") {
            let name = path
                .file_name()
                .expect("file name")
                .to_string_lossy()
                .into_owned();
            let content = fs::read_to_string(&path).expect("read generated file");
            files.insert(name, content);
        }
    }
    files
}

#[test]
fn checked_in_ts_types_are_fresh() {
    let checked_in_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../frontend/src/gen");
    let temp_dir = std::env::temp_dir().join(format!(
        "posthaste-client-models-freshness-{}",
        std::process::id()
    ));
    if temp_dir.exists() {
        fs::remove_dir_all(&temp_dir).expect("clear temp export dir");
    }

    posthaste_client_models::codegen::export_into(&temp_dir).expect("export into temp dir");
    let expected = read_ts_files(&temp_dir);
    let actual = read_ts_files(&checked_in_dir);
    fs::remove_dir_all(&temp_dir).expect("remove temp export dir");

    let mut problems = Vec::new();
    for (name, content) in &expected {
        match actual.get(name) {
            None => problems.push(format!("missing: {name}")),
            Some(checked_in) if checked_in != content => {
                problems.push(format!("stale: {name}"));
            }
            Some(_) => {}
        }
    }
    for name in actual.keys() {
        if !expected.contains_key(name) {
            problems.push(format!("orphaned: {name}"));
        }
    }

    assert!(
        problems.is_empty(),
        "frontend/src/gen/ is out of date with the models crate — run `just gen-ts`:\n  {}",
        problems.join("\n  ")
    );
}
