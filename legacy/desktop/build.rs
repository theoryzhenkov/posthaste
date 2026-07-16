fn main() {
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_E2E_TESTING");
    println!("cargo:rerun-if-changed=capabilities/default.json");
    println!("cargo:rerun-if-changed=capabilities/e2e-playwright.json");

    // Resolve the release channel here and re-export it as a build-script env so
    // `lib.rs` reads it via `env!`. Going through the build script (rather than
    // `option_env!` directly) makes the channel a first-class cargo build input:
    // `rerun-if-env-changed` forces a rebuild when the channel changes, so a
    // stale `target/`/sccache object can never silently carry the wrong channel.
    println!("cargo:rerun-if-env-changed=POSTHASTE_RELEASE_CHANNEL");
    let channel = std::env::var("POSTHASTE_RELEASE_CHANNEL").unwrap_or_else(|_| "dev".to_string());
    println!("cargo:rustc-env=POSTHASTE_RELEASE_CHANNEL_RESOLVED={channel}");

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
