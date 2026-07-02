{
  pkgs,
  system,
  theor-project,
}:

# Toolchain for the client-layer WASM replica (docs/replication/L2, L3):
# building posthaste-client-node-wasm to wasm32-unknown-unknown and generating its JS
# bindings. Chromium for launching Posthaste is already provided by the
# playwright browsers in posthaste.nix.
{
  packages = [
    # The wasm32 link step needs a wasm linker (`wasm-ld`); the cdylib otherwise
    # fails with `linker `lld` not found`. lld provides wasm-ld + lld.
    pkgs.lld
    # Generates the JS loader + .d.ts from the built .wasm. Its version is
    # version-locked to the `wasm-bindgen` crate, which is pinned to match it
    # (=0.2.117) in crates/posthaste-client-node-wasm/Cargo.toml — bump both together.
    pkgs.wasm-bindgen-cli
    # wasm-opt, for release size optimization of the generated module.
    pkgs.binaryen
  ];

  shellHook = ''
    # The replica wasm bundle is built explicitly (it is excluded from the native
    # workspace build). Build + bindgen with:
    #   cargo build -p posthaste-client-node-wasm --release --target wasm32-unknown-unknown
    #   wasm-bindgen target/wasm32-unknown-unknown/release/posthaste_client_node_wasm.wasm \
    #     --out-dir apps/web/src/runtime/wasm --target web
    :
  '';
}
