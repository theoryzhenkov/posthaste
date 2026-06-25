# Posthaste

Your mail, delivered at Posthaste.

[posthaste.theor.net](https://posthaste.theor.net)

> **Beta status.** Posthaste is a local-first, open-source mail workstation in
> active development. Dogfood builds are available now; the public beta is
> invite-and-download for technical early adopters. Expect sharp edges, and
> please report bugs in the [releases discussion](https://github.com/theoryzhenkov/posthaste/discussions/categories/releases).

## Description

Posthaste is a modern, fast email workstation with extensive capabilities. Posthaste is multifaceted, made as accessible as a power client can be, but it is not for everyone. Its users love it for:

- **Agent-first, modular design**: Posthaste is built from composable pieces, and can be assembled as one sees fit. You can download the batteries-included app version, or run Posthaste's backend on your own server, stream the localhost endpoint on a private tailnet, and connect many clients to one backend instance to share drafts, smart mailboxes, and other state. You can run Posthaste's backend as a daemon, allowing agents and scripts to communicate with it via an OpenAPI-compatible API or a provided MCP server, or subscribe to its event stream to trigger custom code when important mail arrives. Since Posthaste is fully open source, you can also fork it and modify it as you see fit.
- **Speed and power**: Posthaste comes with built-in smart mailboxes with advanced queries, and an action system, allowing you to route, classify and manage your email more efficiently. Its command palette is always one click away and suggests context-relevant actions so that you get done with your mail faster. It is written with an efficient Rust backend, a TypeScript Tauri frontend, with minimal algorithmic complexity, minimizing latency even with giant mail sources.
- **Active support**: Posthaste is in active development, integrating user requests and bug reports as quickly as possible. This means you will never have to wait long for everything you need to be available in Posthaste. Future plans even include extending support to JMAP-backed calendars!
- **JMAP**: Posthaste originated as a JMAP-compatible version of MailMate. Over time, it evolved into something more than that, but native JMAP support is still something I'm proud of. You can migrate to JMAP via registering on Fastmail or with a self-hosted Stalwart mail server.

## Installation

Download the latest release for your platform from the [releases page](https://github.com/theoryzhenkov/posthaste/releases).

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

macOS release builds are signed with an Apple Developer ID and notarized. On
first launch, macOS may still show a security prompt; choose **Open** to allow the
app. If Gatekeeper blocks the app entirely, it means the build was not produced
from an official release: do not install unsigned builds from untrusted sources.

### Windows

Download the NSIS installer `.exe` and run it. Windows builds are currently
unsigned; SmartScreen may show a warning on first run. Choose **More info** →
**Run anyway** if you trust the release.

## Updates

Desktop releases include an in-app updater on Linux, macOS, and Windows. The app
checks for updates on launch and offers a one-click install-and-restart when a
new dogfood release is available.

If auto-update is disabled or unavailable, download the latest release manually
from the releases page.

## Supported providers

The primary supported provider path is **JMAP** (Fastmail, Stalwart). IMAP/SMTP
support exists but is provider-sensitive and considered beta-limited. See the
release notes for the current provider matrix and known caveats.

## Support

- Report bugs and discuss releases in the [releases discussion](https://github.com/theoryzhenkov/posthaste/discussions/categories/releases).
- Include the app version, platform, and a description of what you were doing.
- Diagnostic logs may be requested; the app logs to the OS-specific log directory
  (see the release notes for exact paths on each platform).

## Development

See the repository [`docs/`](docs/) tree and the [PostHaste Lab](tools/lab/)
suite for developer workflows, specs, and integration-test tooling.

## License

Posthaste is distributed under an MIT license.