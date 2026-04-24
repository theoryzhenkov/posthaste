# Root justfile - orchestrates backend, frontend, desktop, docs, and dev stacks

mod template
mod mkdocs
mod backend 'crates/justfile'
mod frontend 'apps/web/justfile'
mod site 'apps/site/justfile'
mod desktop 'apps/desktop/justfile'
mod docs 'docs/justfile'

default:
    @just --list

# Phase 2: project setup (run inside the flake devShell, after ./bootstrap.sh)
setup:
    ./setup.sh

# Format all code
fmt:
    just backend fmt
    just frontend fmt
    just site fmt

# Check formatting without modifying files
fmt-check:
    just backend fmt-check
    just frontend fmt-check
    just site fmt-check

# Smoke dev wiring, lint, typecheck, format-check, and docs build
check:
    just dev-smoke
    just fmt-check
    just backend check
    just frontend check
    just site check
    just docs build

# Run all tests
test *args:
    just backend test {{ args }}
    just frontend test

# Build everything
build:
    just backend build
    just frontend build
    just desktop build
    just site build

# Build the browser-localhost distributable assets and server binary.
build-serve:
    just frontend build
    just backend build-release

# Create a local browser-localhost tarball under target/distribute/.
package-serve:
    just build-serve
    bash tools/package/serve.sh

# Run browser-localhost mode from the built frontend.
serve *args:
    cargo run --bin posthaste -- serve --frontend-dist apps/web/dist {{ args }}

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

# Start Stalwart + seed + server + Vite with Overmind.
dev-web:
    bash tools/dev/overmind/launch.sh web

# Start Stalwart + seed + Tauri with Overmind.
dev-desktop:
    bash tools/dev/overmind/launch.sh desktop

# Start Stalwart + seed + server with Overmind.
dev-services:
    bash tools/dev/overmind/launch.sh services

# Validate dev stack paths and recipe wiring without starting services.
dev-smoke:
    bash tools/dev/smoke.sh

# --- Local Stalwart dev server (end-to-end testing) ---
# See tools/dev/stalwart/ for config and seed script.
# Full-stack browser dev: just dev-web
# Full-stack desktop dev: just dev-desktop
# Services only: just dev-services
# Vite only: just frontend dev
# Tauri only: just desktop dev

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
SERVER_LOG_PATH_SCRIPT := justfile_directory() / "tools/dev/overmind/server-log-path.sh"

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

# Print the current or expected persisted server log path for dev.
server-log-path:
    @{{ SERVER_LOG_PATH_SCRIPT }}

# Follow the persisted server log file used by the dev stack.
server-log-tail:
    @tail -F "$({{ SERVER_LOG_PATH_SCRIPT }})"

# Query the persisted JSONL server log. Examples:
#   just server-log-query --account local-stalwart --message "sync completed"
#   just server-log-query --sync-id 6f2a4a72-0c59-4d89-9d4e-2a2b9f2c4a87
#   just server-log-query --target posthaste_imap --json --limit 20
server-log-query *args:
    @bash tools/dev/logs/query.sh {{ args }}
