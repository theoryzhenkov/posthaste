{
  description = "Posthaste dev shell";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, rust-overlay, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs { inherit system overlays; };

        rustToolchain = pkgs.rust-bin.stable.latest.default.override {
          extensions = [ "rust-src" "rust-analyzer" ];
        };
      in
      {
        devShells.default = pkgs.mkShell {
          nativeBuildInputs = [
            # Repository tooling
            pkgs.git
            pkgs.jujutsu
            pkgs.just
            pkgs.sops
            pkgs.age

            # Rust
            rustToolchain
            pkgs.cargo-tauri
            pkgs.pkg-config

            # Node / frontend
            pkgs.nodejs_22
            pkgs.bun

            # Docs (mkdocs-material via Python)
            pkgs.python3
            pkgs.python3Packages.mkdocs-material

            # Tauri 2 build deps (Linux)
          ] ++ pkgs.lib.optionals pkgs.stdenv.hostPlatform.isLinux [
            pkgs.webkitgtk_4_1
            pkgs.libsoup_3
            pkgs.gtk3
            pkgs.glib-networking
            pkgs.openssl
            pkgs.libayatana-appindicator
          ];

          buildInputs = pkgs.lib.optionals pkgs.stdenv.hostPlatform.isLinux [
            pkgs.webkitgtk_4_1
            pkgs.libsoup_3
            pkgs.gtk3
            pkgs.openssl
          ];

          shellHook = ''
            FLAKE_ROOT="$PWD"
            while [ "$FLAKE_ROOT" != "/" ] && [ ! -f "$FLAKE_ROOT/flake.nix" ]; do
              FLAKE_ROOT="$(dirname "$FLAKE_ROOT")"
            done

            if [ ! -f "$FLAKE_ROOT/flake.nix" ]; then
              FLAKE_ROOT="$PWD"
            fi

            export FLAKE_ROOT
            export SOPS_AGE_KEY_FILE="$FLAKE_ROOT/.age-key"
            export GIO_EXTRA_MODULES="${pkgs.lib.optionalString pkgs.stdenv.hostPlatform.isLinux
              "${pkgs.glib-networking}/lib/gio/modules"}"
          '';
        };
      }
    );
}
