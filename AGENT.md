# Agent Instructions

## Project

PostHaste is a JMAP mail client with a Rust backend, local SQLite replica,
Tauri desktop shell, and React/TypeScript frontend.

## Conventions

- Use `jj` for version control. See `jj log` for history.
- Use `just` for task running. See `just --list` for available commands.
- Template updates use `just template update` and require a jj repository. Derived template setup adds the parent template as the `template` git remote; if setup has not run, add it manually: `git remote add template git@github.com:theoryzhenkov/repo_template.base_mkdocs.git`.
- MkDocs serves documentation from `docs/`; use `just mkdocs serve` and `just mkdocs build`.
- Secrets are managed with `sops` + `age`. Never commit `.env` or `.age-key`.
- Documentation follows the [SPECial](https://the-o-space.github.io/special/) standard. See `special.conf.toml`.
