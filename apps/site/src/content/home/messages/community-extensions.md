---
id: community-extensions
from: Posthaste
subject: Your mail has an API
tag: api
time: '09:05'
color: violet
---

The same Rust backend that powers the app exposes a versioned local API: REST under `/v1` (OpenAPI), a live event stream over SSE (AsyncAPI), and an MCP adapter.

Write another client. Subscribe to mail events and wire up local automations. Point a trusted local agent at a real mail interface.

The MCP adapter is early and trusted-local only — the daemon token grants broad access until capability scoping lands.
