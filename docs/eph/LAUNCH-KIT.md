# Launch kit (internal) — beta launch assets

> Copy-paste assets for the first-few-dozen-users open-source beta. **Fire these only
> once the build is a stable, one-click-installable beta** (mail-safety follow-up +
> field-test done) — the assets are worthless if the first users hit mail loss.
> Everything below is grounded in real capabilities; verify the flagged items (⚠) before posting.

---

## GitHub repo — description + topics

**Description** (under the repo name, keep it one line):
> Local-first mail client your scripts and AI agents can actually use — an MCP server + rules engine over Gmail, JMAP & IMAP. Built in Rust. (Beta)

**Topics** (Settings → Topics; up to 20 — GitHub discovery + awesome-lists key off these):
`email` · `email-client` · `mail-client` · `local-first` · `mcp` · `model-context-protocol` · `ai-agents` · `agents` · `automation` · `rust` · `jmap` · `imap` · `gmail` · `desktop-app` · `privacy` · `tauri` ⚠(only if it's actually Tauri)

---

## Post 1 — MCP directory / awesome-mcp-servers (Tier 1, do first)

**One-line (awesome-mcp-servers PR format — `- [Name](url) 🖥️ — desc`):**
> - [Posthaste](https://github.com/theoryzhenkov/posthaste) 🖥️ — Local-first desktop mail client that exposes your inbox as an MCP server: agents read, search, tag, and reply to mail with scoped, auto-expiring tokens. Gmail / JMAP / IMAP.

**Paragraph (for directories with a description field, e.g. mcp.so):**
> **Posthaste** is a local-first, multi-provider desktop mail client (Gmail, Fastmail/JMAP, self-hosted IMAP) that ships an **MCP server** — so an agent can operate on your real mailbox: read and search messages, apply tags, move, and reply, all through scoped tokens that expire, without handing over your account credentials. A server-side rules engine can also trigger agents on incoming mail (`tag → agent → action`). Your mail stays on your machine in a fast optimistic local replica. Open source, in beta.

**Where:** `punkpeye/awesome-mcp-servers`, `wong2/awesome-mcp-servers`, `mcp.so`, the modelcontextprotocol servers registry. One PR/submission each. (This is your warmest, most-differentiated audience — lead here.)

---

## Post 2 — r/rust `[Media]` post

**Title:**
> [Media] Posthaste — a local-first mail client in Rust that exposes your inbox as an MCP server for AI agents

**Image:** attach `apps/site/public/screenshots/command-palette.png` (r/rust Media posts are image-first).

**Body:**
> I've been building **Posthaste**, a local-first desktop mail client written in Rust. Two things make it different from the usual clients:
>
> 1. **It's local-first.** Your mail replicates into a fast on-device store (an optimistic replica in a WASM worker), so reads work offline and every action feels instant. Providers: Gmail (OAuth), Fastmail/JMAP, and self-hosted IMAP.
> 2. **It's programmable by design.** Everything the app does is available over the same documented API — REST, a CLI (`posthastectl`), and an **MCP server**. You can point an AI agent at your inbox (read/tag/reply via scoped, auto-expiring tokens), or wire a rules engine to trigger scripts/webhooks/agents when mail lands.
>
> The stack: a Rust workspace (authority server, runtime, provider gateways, a SQLite store), a WASM optimistic replica, and a TS/React client.
>
> It's an **early beta** — expect sharp edges, and I'd love feedback (and bug reports). Repo + downloads: https://github.com/theoryzhenkov/posthaste
>
> Happy to answer anything about the architecture — the local-first replication and the "one vocabulary for the UI, the CLI, and agents" design were the fun parts.

**Also:** submit to **This Week in Rust** (the "Call for Participation" / project spotlight form) — one email, whole-community reach.

---

## Post 3 — Bluesky / Mastodon

**Bluesky (~300 chars) / Mastodon (can be longer, add more hashtags):**
> I built Posthaste 📮 — a local-first mail client (Gmail · JMAP · IMAP) that exposes your inbox as an **MCP server**, so your scripts and AI agents can read, tag, and reply to mail with scoped tokens. Rust + an optimistic on-device replica. Early beta, feedback very welcome 👇
>
> https://github.com/theoryzhenkov/posthaste
>
> #localfirst #mcp #rustlang #email

(Attach the screenshot. On Mastodon add `#opensource #selfhosting`.)

---

## Passive channels (submit once, trickle stars for months)
Awesome-list PRs: **awesome-rust** (Applications → Email), **awesome-email**, **awesome-local-first-software**, **awesome-selfhosted** ⚠(only if you lean the self-host/JMAP-server angle). Same one-line as Post 1.

## Sequencing
1. Prep done (this file) → fill the ⚠ items.
2. Cut a **stable, installable beta** + Starlight docs live.
3. Fire **Tier-1** (MCP directories + awesome-lists) — passive, warm, low-risk.
4. Then **r/rust `[Media]` + This Week in Rust + Bluesky/Mastodon** (one push each).
5. **Hold** Show HN / Product Hunt for the proven build (one shot each).

## ⚠ Verify before posting
- Tauri? (topic) — confirm the desktop framework before tagging `tauri`.
- Discord/community link — the README references one; confirm it's live before pointing people at it.
- Gmail OAuth is confirmed supported (XOAUTH2, per the onboarding audit). Fastmail/JMAP + self-hosted IMAP confirmed.
- Screenshot is current with the shipped UI.
</content>
