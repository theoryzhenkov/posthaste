//! Writes the generated TypeScript protocol types into
//! `apps/client/frontend/src/gen/`. Run as `just gen-ts` (or `cargo run -p
//! posthaste-client-models --bin export-ts`).

use std::path::Path;
use std::process::ExitCode;

fn main() -> ExitCode {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../frontend/src/gen");
    match posthaste_client_models::codegen::export_into(&dir) {
        Ok(()) => {
            println!("exported TypeScript protocol types to {}", dir.display());
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("export failed: {error}");
            ExitCode::FAILURE
        }
    }
}
