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
