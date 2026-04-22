# Agent Instructions

## Project

<!-- Describe the project here. -->

## Conventions

- Use `jj` for version control. See `jj log` for history.
- Use `just` for task running. See `just --list` for available commands.
- Template updates use `just template update` and require a jj repository.
- Secrets are managed with `sops` + `age`. Never commit `.env` or `.age-key`.
- Documentation follows the [SPECial](https://the-o-space.github.io/special/) standard. See `special.conf.toml`.
