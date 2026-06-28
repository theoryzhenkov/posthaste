---
scope: L2
summary: "Companion to RFC-L2-configuration-surface: the per-parameter decision matrix (tier × scope × home × blast radius) and the proposed TOML schema additions. Revised — TOML is the single source of truth (incl. UI prefs); only ephemeral device geometry stays local; no telemetry."
modified: 2026-06-28
reviewed: 2026-06-28
state: planned
depends:
  - path: docs/eph/RFC-L2-configuration-surface
    local: "1. Decision matrix"
  - path: docs/L1-accounts
    section: "TOML schema"
    local: "2. Proposed TOML schema additions"
---

# RFC Appendix — Configuration Decision Matrix

Companion to [RFC-L2-configuration-surface](RFC-L2-configuration-surface.md).
Tiers: **B**=UI-basic · **A**=UI-advanced · **T**=TOML-only · **H**=hidden.
Home: `toml` (file source of truth) / `local` (ephemeral per-device view-state,
never a file) / `—` (derived). Blast: safe / confirm / **destr**. ✚ = new or
newly-exposed. The UI *tier* column targets the P2 settings revamp; the P1
"basics" surfaced first are the B rows under General / Accounts / Storage & Sync /
Troubleshooting.

## 1. Decision matrix [::state planned]

### Global (`app.toml`)
| param | tier | home | repair? | blast | rationale |
|---|---|---|---|---|---|
| `default_source_id` | B | toml | – | safe | common, single select |
| `automations[]`, `draft_automations[]` | A | toml | – | confirm | has editor; bulk/declarative |
| `daemon.bind` | T | toml | – | confirm | startup-only; networking |
| `daemon.cors_origin` | T | toml | – | confirm | startup-only; security |
| `daemon.poll_interval_seconds` | A | toml | – | safe | sync cadence (per-acct override below) |
| `daemon.require_auth` | T | toml | – | **destr** | disabling = unauth local API |
| `daemon.runtime.*` | T | toml | – | confirm | expert "mirror hard-coded defaults"; never UI |
| `logging.level` | A ✚ | toml | yes | safe | diagnostics; in Troubleshooting |
| `cache.soft_cap_bytes`/`hard_cap_bytes` | A | toml | **yes** | safe | the user's "limit cache size" — exists |
| `cache.cache_bodies/raw/attachments` | A | toml | **yes** | safe | what to retain |
| `cache.eviction` ✚ | A | toml | **yes** | safe | lru vs score |
| `cache.body_ttl_days` ✚ | A | toml | **yes** | safe | age-out; 0 = never |
| `link.serve/token/backend_url` | T | toml | – | **destr** | remote topology; mis-set = no backend |
| `storage.data_dir` ✚ | T | toml | **yes** | **destr** | relocates config+sqlite; restart |
| `updates.channel` ✚ | A | toml | – | confirm | stable↔nightly; build-time today (needs updater) |
| `updates.auto_check` ✚ | B | toml | – | safe | UX toggle |
| `appearance.mode/palettePreset/density` | B | toml ✚ | – | safe | **moved from localStorage → TOML** |
| `appearance.accentHue/glassTheme` | A | toml ✚ | – | safe | fine-grained / advanced theme |
| `notifications.*` (new_mail/sound/mutes) ✚ | B | toml | – | safe | policy in TOML; OS-permission is local |
| `keymap.*` ✚ | A | toml | – | safe | declarative; own `keymap.toml` |
| `startup.restore_last_view` ✚ | B | toml | – | safe | UX |
| `startup.launch_at_login` ✚ | B | local | – | safe | OS integration; per-device |
| `schema_version` | H | – | – | – | managed by loader |

### Per-account (`sources/{id}.toml`)
| param | tier | home | repair? | blast | rationale |
|---|---|---|---|---|---|
| `name`, `full_name` | B | toml | – | safe | identity/display |
| `identity.signature` ✚ | B | toml | – | safe | compose signature (new) |
| `enabled` | B | toml | – | confirm | pauses sync |
| `driver` | B | toml | – | confirm | set at setup, then locked |
| `email_patterns[]` | A | toml | – | safe | routing |
| `appearance` (initials/image/hue) | B | toml | – | safe | account mark |
| `transport.*` (host/port/security/auth/url/user) | A | toml | – | confirm | connectivity |
| `transport.secret_ref` | A | toml | – | **destr** | credentials; keyring/env |
| `sync.poll_interval_seconds` ✚ | A | toml | – | safe | per-acct cadence override (P3) |
| `sync.retention_days` ✚ | A | toml | **yes** | confirm | data-footprint / sync window (P3) |
| `cache` (per-account caps) ✚ | A | toml | **yes** | safe | override global caps (P3) |
| `id`, `created_at`, `updated_at` | H | – | – | – | managed |

### Per-smart-mailbox (`smart-mailboxes/{id}.toml`)
| param | tier | home | blast | rationale |
|---|---|---|---|---|
| `name` | B | toml | safe | label |
| `position` | B | toml | safe | order (drag in UI) |
| `role` ✚ | A | toml | confirm | **expose** — drives contextual actions; bad role = wrong destructive verbs |
| `parent_id` | A | toml | safe | nesting |
| `rule` (groups/conditions) | A | toml | safe | rule editor |
| `kind`, `default_key`, `id`, timestamps | H | – | – | managed/derived |

### Ephemeral device view-state (`localStorage` — never a file, never in settings)
| param | tier | home | rationale |
|---|---|---|---|
| `developerToolsEnabled` | A | local | General → Advanced toggle |
| thread-list `columns/sort/widths` | H | local | configured in-situ |
| panel layout / floating geometry | H | local | in-situ |

## 2. Proposed TOML schema additions [::state planned]

All new sections are optional tables (`Option<_>` + `#[serde(default)]`) → old
files parse unchanged. `@spec docs/L1-accounts#toml-schema`. **Bump
`schema_version` 1→2** (self-documenting; no destructive migration). The P1
config service must write via `toml_edit` and **preserve unknown sections** (RFC
§6.6) so no section is dropped on save, and **migrate the existing `localStorage`
appearance prefs into `[appearance]` once** on first run.

```toml
# app.toml
schema_version = 2

[storage]                      # ✚ config + mail.sqlite root (TOML knob)
data_dir = "/path/..."
# NOTE: the webview/IndexedDB replica is co-located under the app data dir at
# LAUNCH (Tauri webview data dir), NOT a hot TOML knob — see RFC §4.

[cache]                        # existing, extended
soft_cap_bytes = 1073741824
hard_cap_bytes = 2147483648
cache_bodies = true
cache_raw_messages = false
cache_attachments = true
eviction = "score"             # ✚ "lru" | "score"
body_ttl_days = 0              # ✚ 0 = never

[updates]                      # ✚
channel = "stable"             # "stable" | "nightly" (needs updater support)
auto_check = true

[appearance]                   # ✚ moved from localStorage → TOML (source of truth)
mode = "system"                # "light" | "dark" | "system"
palette_preset = "..."
density = "comfortable"
accent_hue = 250
# glass_theme = { ... }        # advanced theme params

[notifications]                # ✚ policy (OS delivery permission stays device-local)
new_mail = true
sound = true

[logging]
level = "info"
```

```toml
# sources/{id}.toml — additions
[identity]
signature = "…"                # ✚

[sync]                         # ✚ per-account override of global cadence (P3)
poll_interval_seconds = 60
retention_days = 0

[cache]                        # ✚ per-account override of global caps (P3)
soft_cap_bytes = 268435456
hard_cap_bytes = 536870912
```

```toml
# smart-mailboxes/{id}.toml — NO schema change; field already exists
role = "archive"               # ✚ now writable for user mailboxes (validate vs known roles)
```

`keymap` is large + declarative → its **own file** (`keymap.toml`), not an
`app.toml` table, to keep `app.toml` legible. All of `appearance` /
`notifications` / `keymap` are file-backed (TOML), editable by hand or an LLM
without a GUI harness — only ephemeral per-device geometry stays in
`localStorage`.
