# Root justfile - orchestrates backend, frontend, docs, and dev stacks

mod template
mod backend 'crates/justfile'
mod site 'apps/site/justfile'
mod desktop 'apps/client/desktop/justfile'
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

# The client frontend's charter ratchet (docs/client/L2-charter.md §The
# ratchet): tree shape, import boundaries, structural lint, dead code,
# duplication — all fail on NEW violations only, against committed baselines.
client-charter:
    bun --cwd=apps/client/frontend run check:charter

# Format all code
fmt:
    just backend fmt
    just site fmt

# Check formatting without modifying files
fmt-check:
    just backend fmt-check
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
    just site check
    just docs build

# Run all tests
test *args:
    bash tools/disk-guard.sh
    just backend test {{ args }}

# Build everything
build:
    bash tools/disk-guard.sh
    just backend build
    just site build

# Report quota usage + every target/ dir under the checkout (the recurring hog).
disk:
    bash tools/disk-guard.sh report

# Reclaim disk: clean this workspace's Rust target/ (regenerable build cache).
reclaim:
    bash tools/disk-guard.sh clean

# Regenerate the client's TypeScript protocol types (apps/client/protocol/src/gen/)
# from the Rust models crate. Committed output; the models crate's freshness
# test fails when this is stale.
gen-ts:
    cargo run -p posthaste-client-models --bin export-ts

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
# Canonical services stack (Stalwart + seed + mock Gmail): just dev services
# Canonical client frontend dev: bun run client:dev
# Canonical client desktop dev: bun run client:desktop:dev

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
