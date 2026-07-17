# Root justfile - orchestrates backend, frontend, desktop, docs, and dev stacks

mod template
mod backend 'crates/justfile'
mod web 'legacy/web/justfile'
mod mcp 'apps/mcp/justfile'
mod site 'apps/site/justfile'
mod desktop 'legacy/desktop/justfile'
mod dev 'tools/dev/justfile'
mod lab 'tools/lab/justfile'
mod docs 'docs/justfile'

default:
    @just --list

# Project setup (run inside the flake devShell).
setup:
    ./setup.sh

# Install JavaScript workspace dependencies
install:
    bun install

# Format all code
fmt:
    just backend fmt
    just web fmt
    just mcp fmt
    just site fmt

# Check formatting without modifying files
fmt-check:
    just backend fmt-check
    just web fmt-check
    just mcp fmt-check
    just site fmt-check

# Validate a Posthaste config directory. No production config is committed, so
# this is an operator/agent command rather than a default CI input.
validate-config dir:
    cargo run -p posthaste-lab -- config validate --config-dir {{ dir }}

# Smoke dev wiring, lint, typecheck, format-check, and docs build
check:
    just dev smoke
    just fmt-check
    just backend check
    just web check
    just mcp check
    just site check
    just docs build

# Run all tests
test *args:
    bash tools/disk-guard.sh
    just backend test {{ args }}
    just desktop test
    just web test
    just mcp test

# Build everything
build:
    bash tools/disk-guard.sh
    just backend build
    just web build
    just desktop build
    just site build

# Report quota usage + every target/ dir under the checkout (the recurring hog).
disk:
    bash tools/disk-guard.sh report

# Reclaim disk: clean this workspace's Rust target/ (regenerable build cache).
reclaim:
    bash tools/disk-guard.sh clean

# Build the client node WASM bundle (posthaste-client-node-wasm: kernel +
# projector + near-end, RFC-L2-architecture-cleanup D41/D43) and emit the JS
# loader + .d.ts into legacy/web/src/runtime/wasm/. The replicaAdapter loads
# these only when VITE_RUNTIME_REPLICA is enabled. The artifacts are generated
# but committed (like legacy/web/src/api/schema.gen.ts) so web builds need no Rust
# toolchain; re-run this and commit the result after changing the boundary, and
# CI re-runs it to verify the bindings are fresh.
build-client-node-wasm:
    cargo build -p posthaste-client-node-wasm --release --target wasm32-unknown-unknown
    wasm-bindgen target/wasm32-unknown-unknown/release/posthaste_client_node_wasm.wasm \
        --out-dir legacy/web/src/runtime/wasm --target web
    # Skip wasm-opt when SKIP_WASM_OPT is set (e.g. CI smoke tests where the
    # available binaryen version produces a table-max that is incompatible with
    # the committed wasm-bindgen JS glue). Release builds still optimize.
    if [ -z "${SKIP_WASM_OPT:-}" ]; then \
        wasm-opt -Oz legacy/web/src/runtime/wasm/posthaste_client_node_wasm_bg.wasm \
            -o legacy/web/src/runtime/wasm/posthaste_client_node_wasm_bg.wasm; \
    fi

# Regenerate the client's TypeScript protocol types (apps/client/frontend/src/gen/)
# from the Rust models crate. Committed output; the models crate's freshness
# test fails when this is stale.
gen-ts:
    cargo run -p posthaste-client-models --bin export-ts

# Build the browser-localhost distributable assets and server binary.
build-serve:
    just web build
    just backend build-release

# Package the standalone daemon binary (posthasted) under target/distribute/.
package-daemon:
    just backend build-release
    bash tools/package/daemon.sh

# Package the browser-localhost web frontend under target/distribute/.
package-web:
    just web build
    bash tools/package/web.sh

# Run browser-localhost mode from the built frontend.
serve *args:
    cargo run --bin posthaste-authority-runtime-server -- serve --frontend-dist legacy/web/dist {{ args }}

# Print the browser automation environment exposed by the dev shell.
browser-env:
    @echo "PLAYWRIGHT_BROWSERS_PATH=${PLAYWRIGHT_BROWSERS_PATH:-}"
    @echo "PLAYWRIGHT_NODEJS_PATH=${PLAYWRIGHT_NODEJS_PATH:-}"
    @echo "POSTHASTE_PLAYWRIGHT_CLI=${POSTHASTE_PLAYWRIGHT_CLI:-}"

# Run Playwright through the Nix-provided CLI/runtime from the current dev shell.
browser-playwright *args:
    node "${POSTHASTE_PLAYWRIGHT_CLI}" {{ args }}

# Capture a browser screenshot using the shared Playwright runtime.
browser-screenshot url file *args:
    node "${POSTHASTE_PLAYWRIGHT_CLI}" screenshot {{ args }} {{ url }} {{ file }}

# --- Local Stalwart dev server (end-to-end testing) ---
# See tools/dev/stalwart/ for config and seed script.
# Canonical full-stack browser dev: just dev web
# Canonical full-stack desktop dev: just dev desktop
# Canonical services only: just dev services
# Canonical Vite only: just web dev
# Canonical Tauri only: just desktop dev

# Admin password for Stalwart's fallback-admin + dev mailbox password.
# Override with `just stalwart-up admin=... user=...` or set env vars directly.
STALWART_ADMIN_PASSWORD := env_var_or_default("POSTHASTE_STALWART_ADMIN_PASSWORD", "devadmin")
STALWART_USER_PASSWORD := env_var_or_default("POSTHASTE_STALWART_USER_PASSWORD", "devpass")
STALWART_HTTP_BIND := env_var_or_default("POSTHASTE_STALWART_BIND", "127.0.0.1:8080")
STALWART_HTTP_URL := env_var_or_default("POSTHASTE_STALWART_URL", "http://127.0.0.1:8080")
STALWART_IMAP_BIND := env_var_or_default("POSTHASTE_STALWART_IMAP_BIND", "127.0.0.1:1143")
STALWART_SMTP_BIND := env_var_or_default("POSTHASTE_STALWART_SMTP_BIND", "127.0.0.1:1587")
STALWART_DATA := justfile_directory() / "var/dev/stalwart/data"
STALWART_LOGS := justfile_directory() / "var/dev/stalwart/logs"

# Start Stalwart in the foreground. Ctrl-C to stop.
stalwart-up:
    POSTHASTE_STALWART_DATA={{ STALWART_DATA }} \
        POSTHASTE_STALWART_LOGS={{ STALWART_LOGS }} \
        POSTHASTE_STALWART_ADMIN_PASSWORD={{ STALWART_ADMIN_PASSWORD }} \
        POSTHASTE_STALWART_BIND={{ STALWART_HTTP_BIND }} \
        POSTHASTE_STALWART_URL={{ STALWART_HTTP_URL }} \
        POSTHASTE_STALWART_IMAP_BIND={{ STALWART_IMAP_BIND }} \
        POSTHASTE_STALWART_SMTP_BIND={{ STALWART_SMTP_BIND }} \
        stalwart -c tools/dev/stalwart/config.toml

# Provision the dev domain + mailbox user. Idempotent.
stalwart-seed:
    POSTHASTE_STALWART_ADMIN_PASSWORD={{ STALWART_ADMIN_PASSWORD }} \
    POSTHASTE_STALWART_USER_PASSWORD={{ STALWART_USER_PASSWORD }} \
        bash tools/dev/stalwart/seed.sh

# Wipe Stalwart data + logs for a clean slate.
stalwart-reset:
    rm -rf {{ STALWART_DATA }} {{ STALWART_LOGS }}

# Print export lines that point posthaste at the local Stalwart.
# Usage: eval $(just stalwart-dev)
stalwart-dev:
    @echo 'export POSTHASTE_BOOTSTRAP_PATH={{ justfile_directory() }}/tools/dev/bootstrap.stalwart.toml'
    @echo 'export POSTHASTE_STALWART_USER_PASSWORD={{ STALWART_USER_PASSWORD }}'
    @echo 'export POSTHASTE_STALWART_IMAP_BIND={{ STALWART_IMAP_BIND }}'
    @echo 'export POSTHASTE_STALWART_SMTP_BIND={{ STALWART_SMTP_BIND }}'
