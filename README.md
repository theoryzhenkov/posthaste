# Posthaste

Your mail, delivered at Posthaste.

[posthaste.theor.net](https://posthaste.theor.net)

> **Beta status.** Posthaste is a local-first, open-source mail workstation in
> active development. Nightly builds are available now for technical early
> adopters. Expect sharp edges, and please report bugs using the
> [bug-report template](https://github.com/theoryzhenkov/posthaste/issues/new/choose)
> or in the
> [releases discussion](https://github.com/theoryzhenkov/posthaste/discussions/categories/releases).
> See [Beta caveats](#beta-caveats) for what works and what is still limited.

## Description

Posthaste is a modern, fast email workstation with extensive capabilities. Posthaste is multifaceted, made as accessible as a power client can be, but it is not for everyone. Its users love it for:

- **Agent-first, modular design**: Posthaste is built from composable pieces, and can be assembled as one sees fit. You can download the batteries-included app version, or run Posthaste's backend on your own server, stream the localhost endpoint on a private tailnet, and connect many clients to one backend instance to share drafts, smart mailboxes, and other state. You can run Posthaste's backend as a daemon, allowing agents and scripts to communicate with it via an OpenAPI-compatible API or a provided MCP server, or subscribe to its event stream to trigger custom code when important mail arrives. Since Posthaste is fully open source, you can also fork it and modify it as you see fit.
- **Speed and power**: Posthaste comes with built-in smart mailboxes with advanced queries, and an action system, allowing you to route, classify and manage your email more efficiently. Its command palette is always one click away and suggests context-relevant actions so that you get done with your mail faster. It is written with an efficient Rust backend, a TypeScript Tauri frontend, with minimal algorithmic complexity, minimizing latency even with giant mail sources.
- **Active support**: Posthaste is in active development, integrating user requests and bug reports as quickly as possible. This means you will never have to wait long for everything you need to be available in Posthaste. Future plans even include extending support to JMAP-backed calendars!
- **JMAP**: Posthaste originated as a JMAP-compatible version of MailMate. Over time, it evolved into something more than that, but native JMAP support is still something I'm proud of. You can migrate to JMAP via registering on Fastmail or with a self-hosted Stalwart mail server.

## Installation

Download the latest release for your platform from the
[releases page](https://github.com/theoryzhenkov/posthaste/releases).

Two release channels coexist as **separate apps** on the same machine, so
installing nightly does not overwrite a stable install:

| Channel | App name | Bundle identifier | macOS signing | Devtools |
| --- | --- | --- | --- | --- |
| **Stable** | Posthaste | `com.posthaste.mail` | Apple Developer ID + notarized | No |
| **Nightly** | PosthasteNightly | `com.posthaste.mail.nightly` | Apple Developer ID + notarized | Yes |

Stable releases are published less often and are the recommended install for
daily use. Nightly builds track `main` closely and include the in-app developer
tools.

### Linux

Download the `.AppImage` for your architecture, make it executable, and run it:

```bash
chmod +x Posthaste_*.AppImage
./Posthaste_*.AppImage
```

No system-wide installation is required. The AppImage bundles the backend and
frontend; no separate server setup is needed unless you want one.

### macOS

Download the `.dmg` for your architecture and drag Posthaste into Applications.

macOS builds — both nightly and stable — are signed with an Apple Developer ID
and notarized. On first launch, macOS may show a security prompt; choose **Open**
to allow the app. If Gatekeeper blocks the app entirely, it was not produced from
an official release: do not install builds from untrusted sources.

> Nightly builds would fall back to ad-hoc (unsigned) signing only if Apple
> credentials were unavailable. With credentials configured, nightlies are
> fully signed and notarized, same as stable; the only channel difference is
> that stable hard-requires signing + notarization and fails the build without
> it.

### Windows

Download the NSIS installer `.exe` and run it. Windows builds are currently
unsigned; SmartScreen may show a warning on first run. Choose **More info** →
**Run anyway** if you trust the release.

## Updates

Desktop builds include an in-app updater (Linux, macOS arm64, and Windows). The
app checks for updates on launch and offers a one-click install-and-restart
when a new release is available. A manual **Check for updates** button lives in
**Settings → General → Updates**.

macOS auto-update covers the arm64 (`darwin-aarch64`) build only; Intel macOS
users update manually.

### Manual update and rollback

If auto-update is unavailable or you want a specific build:

1. Go to the [releases page](https://github.com/theoryzhenkov/posthaste/releases).
2. Find the build you want. Nightly releases are tagged `v0.2.0-nightly.N`;
   stable releases are tagged `v0.2.0`, `v0.2.0-rc.N`, or similar.
3. Download the bundle for your platform and replace the installed app.
4. To roll back, download an earlier release the same way. Because nightly and
   stable are separate apps with distinct identifiers, replacing one does not
   affect the other.

## Supported providers

The primary supported provider path is **JMAP** (Fastmail, Stalwart). IMAP/SMTP
support exists but is provider-sensitive and considered beta-limited — some
providers' edge cases are not yet covered. See the release notes for the current
provider matrix and known caveats.

## Beta caveats

Posthaste is functional for daily mail, but some features are limited or still
in progress:

- **Body search is preview-limited.** `body:` searches match against cached
  message previews and metadata, not every fetched byte of every message.
  Full-text search across all bodies is planned.
- **IMAP/SMTP is beta-limited.** JMAP (Fastmail, Stalwart) is the primary,
  best-supported path. IMAP/SMTP works but is provider-sensitive.
- **No offline mutation queue.** Reads work offline; mutations (move, delete,
  send) require a connection. A queued-offline-mutation mode is planned.

What does work today: send, reply, reply-all, forward, snooze, drafts with
autosave, attachments, read/unread, flag, tags, move/archive/trash/delete, smart
mailboxes, search (preview-limited as noted above), and the local-first replica.

## Support

- Report bugs using the
  [bug-report template](https://github.com/theoryzhenkov/posthaste/issues/new/choose)
  or discuss releases in the
  [releases discussion](https://github.com/theoryzhenkov/posthaste/discussions/categories/releases).
- Always include: app version and release channel (visible in **Settings →
  About**), platform/OS, provider type (JMAP/IMAP), and a description of what
  you were doing.

### Diagnostic logs

The embedded backend writes structured JSON-lines logs, rotated daily, to:

```
<state_root>/logs/posthaste.YYYY-MM-DD
```

`state_root` resolves to, in order:

1. `$POSTHASTE_STATE_ROOT` if set, otherwise
2. `$XDG_DATA_HOME/posthaste` if set, otherwise
3. `~/.local/share/posthaste` (the default on Linux).

So the typical log path is
`~/.local/share/posthaste/logs/posthaste.YYYY-MM-DD`. When a maintainer asks for
logs, attach the relevant dated file.

**Settings → Troubleshooting** also offers repair and reset pathways
(repair-and-restart, reset replica database, factory reset) for when something
goes wrong.

> **Note (Linux/WebKitGTK):** the renderer's IndexedDB replica lives in a
> WebKit-managed location outside the app data directory. Deleting the app
> bundle does not clear it. Use **Settings → Troubleshooting → Reset replica
> database** to clear the local cache; it rebuilds on the next sync.

## Development

See the repository [`docs/`](docs/) tree and the [PostHaste Lab](tools/lab/)
suite for developer workflows, specs, and integration-test tooling.

## License

Posthaste is distributed under an MIT license.
