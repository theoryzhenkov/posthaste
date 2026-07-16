# Posthaste

**Your mail, delivered at Posthaste**

Posthaste is a fast & smart local email workstation. 

[posthaste.theor.net](https://posthaste.theor.net)

> **Beta status.** Posthaste is in active development, heading toward a public
> beta. Nightly builds are available now for technical early adopters — expect
> sharp edges. Report bugs via the
> [bug-report template](https://github.com/theoryzhenkov/posthaste/issues/new/choose)
> or the
> [releases discussion](https://github.com/theoryzhenkov/posthaste/discussions/categories/releases).

## Why Posthaste

Most mail clients are either closed appliances or thin webmail wrappers — you
can't script them, and you can't point an agent at your inbox without handing
a third party your credentials. Posthaste is built the other way around:

- **Local-first.** Your mail lives on your machine in a fast local replica.
  Reads work offline; the UI is optimistic, so actions feel instant.
- **Automatable by design.** A rules engine reacts to incoming mail — tag,
  move, notify, or hand off to a webhook or script — running server-side even
  with every window closed.
- **Agent-connectable, safely.** A built-in MCP server gives an AI agent
  read/write tools over your mail, with scoped, auto-expiring tokens and an
  explicit least-privilege model — no pasted API keys, no standing access by
  default.
- **Yours.** Fully open source (MIT), no telemetry, no data leaves your
  machine unless you wire it somewhere yourself.

## Key features

| Area | What you get |
| --- | --- |
| **Local-first replica** | Mail syncs into a local store; reads work offline; mutations are optimistic and fast. |
| **Multi-account** | Gmail (OAuth), Fastmail and other JMAP servers, self-hosted JMAP (Stalwart), and IMAP/SMTP. |
| **Automations** | A rules engine: match on a search, then tag / move / notify / call a webhook / emit a fact — no code required for the built-in actions. |
| **Agents & scripting** | An MCP server (`posthaste-mcp`) and a CLI (`posthastectl`) built on the same operation registry — an agent host or a shell script gets read/search/tag/move/reply/send, all token-scoped. |
| **Command palette** | Context-relevant actions one keystroke away. |
| **Compose & drafts** | Send, reply, reply-all, forward, autosaving drafts, attachments. |
| **Smart mailboxes & tags** | Saved queries over a query language backed by SQLite, plus keyword tags for triage. |
| **Modular deployment** | Run the batteries-included desktop app, or split the runtime/authority server across machines (e.g. over a private tailnet) and connect multiple clients to one backend. |

## Status

Posthaste is **beta**, heading toward a public release. Nightly builds track
`main`; stable builds are cut less often. Some areas are still limited —
full-text body search is preview-only today, and there's no offline mutation
queue yet (reads work offline, writes need a connection). See the
[releases page](https://github.com/theoryzhenkov/posthaste/releases) for the
current state and provider matrix.

## Install

Download a build for your platform from the
[releases page](https://github.com/theoryzhenkov/posthaste/releases) or
[posthaste.theor.net](https://posthaste.theor.net):

- **macOS** — signed and notarized `.dmg`, both nightly and stable.
- **Windows** — NSIS `.exe` installer (currently unsigned; SmartScreen may warn).
- **Linux** — `.AppImage`; make it executable and run it. No install required.

Nightly and stable ship as **separate apps** (distinct bundle identifiers), so
installing one doesn't touch the other.

### Build from source

Posthaste is a Rust workspace (backend: runtime, authority server, JMAP/IMAP
providers) plus TypeScript/Tauri frontends. See `Cargo.toml`, `package.json`,
and `flake.nix` at the repo root for the toolchain, and `justfile` for the
common dev commands (`just` lists them).

## Automations and agents

Posthaste can react to mail on its own — tag an invoice the moment it lands,
notify you when your boss writes, run your own script, or hand a message to
your AI agent. Connecting an agent over MCP gives it real tools (read, tag,
move, reply, send) behind scoped, auto-expiring tokens, plus an event feed a
rule or a watcher can turn into a trigger.

- **[Automations guide](https://posthaste.theor.net/docs/automations)** — the
  decision guide, built-in rules, and running your own script.
- **[Agents guide](https://posthaste.theor.net/docs/agents)** — plugging in an
  AI agent over MCP, least-privilege grants, and a full worked example.
- **[Scripting quickstart](https://posthaste.theor.net/docs/scripting-quickstart)** —
  the five-minute technical version, with a reference agent loop.

## Docs

Full documentation lives at
**[posthaste.theor.net/docs](https://posthaste.theor.net/docs)**, including the
automations/agents guides above and the scripting security model.

## Architecture

Posthaste splits into a local optimistic replica (the client you interact
with), a runtime that serves views and forwards mutations, and an authority
server that owns durable state and talks to mail providers (JMAP or IMAP)
through a provider gateway layer. These pieces can run bundled in one desktop
process or split across machines. `posthaste-mcp` and `posthastectl` sit on
top of the same documented `/v1` API the app itself uses, so nothing an agent
or script does is a special, undocumented path. For the full picture, see the
architecture notes and RFCs under `docs/`.

## Contributing

Posthaste is open source and takes issues and PRs on
[GitHub](https://github.com/theoryzhenkov/posthaste). Join the
[Discord](https://discord.gg/8ARFrDa2Gv) to discuss features or get help.

## License

MIT — see [`LICENSE`](LICENSE).
