fn main() {
    println!("cargo:rerun-if-changed=capabilities/default.json");

    // Resolve the release channel here and re-export it as a build-script env
    // so `lib.rs` reads it via `env!`. Going through the build script (rather
    // than `option_env!` directly) makes the channel a first-class cargo build
    // input: `rerun-if-env-changed` forces a rebuild when the channel changes,
    // so a stale `target/`/sccache object can never silently carry the wrong
    // channel.
    println!("cargo:rerun-if-env-changed=POSTHASTE_RELEASE_CHANNEL");
    let channel = std::env::var("POSTHASTE_RELEASE_CHANNEL").unwrap_or_else(|_| "dev".to_string());
    println!("cargo:rustc-env=POSTHASTE_RELEASE_CHANNEL_RESOLVED={channel}");

    tauri_build::try_build(
        tauri_build::Attributes::new().capabilities_path_pattern("capabilities/default.json"),
    )
    .expect("failed to build Tauri application context");
}
