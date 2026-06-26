fn main() {
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_E2E_TESTING");
    println!("cargo:rerun-if-changed=capabilities/default.json");
    println!("cargo:rerun-if-changed=capabilities/e2e-playwright.json");

    // `apps/desktop/src/lib.rs` bakes the release channel and its sentinel
    // string into the binary via `option_env!`. Cargo does not watch arbitrary
    // env vars by default, so a stale `target/` cache can silently reuse an
    // object file built for another channel. Declare the dependency so the
    // crate is recompiled whenever the channel/sentinel changes.
    println!("cargo:rerun-if-env-changed=POSTHASTE_RELEASE_CHANNEL");
    println!("cargo:rerun-if-env-changed=POSTHASTE_RELEASE_CHANNEL_SENTINEL");

    let capabilities_path_pattern = if std::env::var_os("CARGO_FEATURE_E2E_TESTING").is_some() {
        "capabilities/*.json"
    } else {
        "capabilities/default.json"
    };

    tauri_build::try_build(
        tauri_build::Attributes::new().capabilities_path_pattern(capabilities_path_pattern),
    )
    .expect("failed to build Tauri application context");
}
