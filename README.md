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

## Scripting & MCP

`posthastectl` is the CLI bundled with the desktop app — on macOS at
`Posthaste.app/Contents/MacOS/posthastectl` (or `PosthasteNightly.app` on the
nightly channel); symlink it onto your `PATH`. It discovers the running app
automatically (no URLs or tokens to copy), runs a handler script per matching
message via `posthastectl watch --exec 'sh ./handler.sh'` (the message arrives
as JSON on stdin), and exposes the same operations to agent hosts via
`posthastectl mcp`. The connection grants the full mail surface — read, write,
and send across every account — so only connect handlers you trust.

## Docs

Full documentation lives at
**[posthaste.theor.net/docs](https://posthaste.theor.net/docs)**.

## Contributing

Posthaste is open source and takes issues and PRs on
[GitHub](https://github.com/theoryzhenkov/posthaste). Join the
[Discord](https://discord.gg/8ARFrDa2Gv) to discuss features or get help.

## License

MIT — see [`LICENSE`](LICENSE).
