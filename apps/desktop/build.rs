fn main() {
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_E2E_TESTING");
    println!("cargo:rerun-if-changed=capabilities/default.json");
    println!("cargo:rerun-if-changed=capabilities/e2e-playwright.json");

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
