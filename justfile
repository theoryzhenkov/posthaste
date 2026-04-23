# Root justfile - orchestrates backend, frontend, desktop, docs, and template tasks

mod template
mod mkdocs
mod backend 'crates/justfile'
mod frontend 'web/justfile'
mod desktop 'src-tauri/justfile'
mod docs 'docs/justfile'

default:
    @just --list

# Phase 2: project setup (run inside the flake devShell, after ./bootstrap.sh)
setup:
    ./setup.sh

# Format all code
fmt:
    just backend fmt
    just frontend lint-fix

# Lint / clippy everything
check:
    just backend check
    just frontend check

# Run all tests
test *args:
    just backend test {{ args }}

# Build everything
build:
    just backend build
    just frontend build

# --- Local Stalwart dev server (end-to-end testing) ---
# See dev/stalwart/ for config and seed script.
# Full-stack browser dev: just frontend dev
# Full-stack desktop dev: just desktop dev

# Admin password for Stalwart's fallback-admin + dev mailbox password.
# Override with `just stalwart-up admin=... user=...` or set env vars directly.
STALWART_ADMIN_PASSWORD := env_var_or_default("POSTHASTE_STALWART_ADMIN_PASSWORD", "devadmin")
STALWART_USER_PASSWORD := env_var_or_default("POSTHASTE_STALWART_USER_PASSWORD", "devpass")
STALWART_DATA := justfile_directory() / "dev/stalwart/data"
STALWART_LOGS := justfile_directory() / "dev/stalwart/logs"
DAEMON_LOG_PATH_SCRIPT := justfile_directory() / "dev/overmind/daemon-log-path.sh"

# Start Stalwart in the foreground. Ctrl-C to stop.
stalwart-up:
    POSTHASTE_STALWART_DATA={{ STALWART_DATA }} \
    POSTHASTE_STALWART_LOGS={{ STALWART_LOGS }} \
    POSTHASTE_STALWART_ADMIN_PASSWORD={{ STALWART_ADMIN_PASSWORD }} \
        stalwart -c dev/stalwart/config.toml

# Provision the dev domain + mailbox user. Idempotent.
stalwart-seed:
    POSTHASTE_STALWART_ADMIN_PASSWORD={{ STALWART_ADMIN_PASSWORD }} \
    POSTHASTE_STALWART_USER_PASSWORD={{ STALWART_USER_PASSWORD }} \
        bash dev/stalwart/seed.sh

# Wipe Stalwart data + logs for a clean slate.
stalwart-reset:
    rm -rf {{ STALWART_DATA }} {{ STALWART_LOGS }}

# Print export lines that point posthaste-daemon at the local Stalwart.
# Usage: eval $(just stalwart-dev)
stalwart-dev:
    @echo 'export POSTHASTE_BOOTSTRAP_PATH={{ justfile_directory() }}/dev/bootstrap.stalwart.toml'
    @echo 'export POSTHASTE_STALWART_USER_PASSWORD={{ STALWART_USER_PASSWORD }}'

# Print the current or expected persisted daemon log path for dev.
daemon-log-path:
    @{{ DAEMON_LOG_PATH_SCRIPT }}

# Follow the persisted daemon log file used by the dev stack.
daemon-log-tail:
    @tail -F "$({{ DAEMON_LOG_PATH_SCRIPT }})"
