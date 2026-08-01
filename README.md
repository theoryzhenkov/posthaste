# Posthaste

**Your mail, delivered at Posthaste**

Posthaste is a fast & smart local email workstation. Posthaste adapts to your workflow, whenever you want smart command palette, custom scripting, MCP server, themes, integrations with your personal tools, and other things. 

[posthaste.theor.net](https://posthaste.theor.net)

## Install

Download a build for your platform from the
[releases page](https://github.com/theoryzhenkov/posthaste/releases) or
[posthaste.theor.net](https://posthaste.theor.net/releases):

- **macOS** — signed and notarized `.dmg`.
- **Windows** — NSIS `.exe` installer.
- **Linux** — `.AppImage`;

Nightly and stable ship as separate apps.

### Build from source

Posthaste is a Rust workspace (backend: a domain service over a SQLite
store with JMAP/IMAP providers) plus a TypeScript/Tauri frontend. See
`Cargo.toml`, `package.json`,
and `flake.nix` at the repo root for the toolchain, and `justfile` for the
common dev commands (`just` lists them).

## Docs

Full documentation lives at
**[posthaste.theor.net/docs](https://posthaste.theor.net/docs)**.

## Contributing

Posthaste is open source and takes issues and PRs on
[GitHub](https://github.com/theoryzhenkov/posthaste). Join the
[Discord](https://discord.gg/8ARFrDa2Gv) to discuss features or get help.

## License

MIT — see [`LICENSE`](LICENSE).
