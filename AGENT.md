# Agent Instructions

## Project

PostHaste is a JMAP mail client with a Rust backend, local SQLite replica,
Tauri desktop shell, and React/TypeScript frontend.

## Conventions

- Use `jj` for version control. See `jj log` for history.
- Use `just` for task running. See `just --list` for available commands.
- Template updates use `just template update`, which runs `copier update` for each `.copier-answers.<layer>.yml` file. Requires `copier` from the flake dev shell.
- To add another Copier template layer to this project: `template legacy-adopt LAYER . -- --defaults` for existing layers, then commit each layer before adopting the next one.
- MkDocs serves documentation from `docs/`; use `just mkdocs serve` and `just mkdocs build`.
- Secrets are managed with `sops` + `age`. Never commit `.env` or `.age-key`.
- Documentation follows the [SPECial](https://the-o-space.github.io/special/) standard. See `special.conf.toml`.
