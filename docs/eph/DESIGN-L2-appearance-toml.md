---
scope: L2
summary: "Migrate UI appearance prefs (theme mode/palette/density/accent/glass) from the renderer's localStorage snapshot into TOML as the single source of truth, with a derived localStorage boot-cache to avoid FOUC. Implements P1.4b of the configuration-surface RFC."
modified: 2026-06-28
reviewed: 2026-06-28
state: implemented
depends:
  - path: docs/eph/RFC-L2-configuration-surface
    local: "Decision: TOML = single source of truth"
  - path: docs/eph/RFC-L2-configuration-matrix
    local: "appearance.* rows + proposed [appearance] schema"
---

# Design — Appearance prefs → TOML (P1.4b)

Implements the `appearance.*` rows of [RFC-L2-configuration-matrix](RFC-L2-configuration-matrix.md).
Moves the renderer's `DesignThemePreferences` (currently an opaque
`localStorage` "client-preferences snapshot") into `[appearance]` in `app.toml`,
so it is LLM/CLI-editable like the rest of the config — no GUI harness required.

## Status [::state implemented]

**Shipped** — backend (`7ae31bfa`) plumbs `[appearance]` through domain→config→API→wire; frontend (`886dd920`) adds the Option A reconcile + write-through + one-time import, reusing the existing `AppearancePane`. 322 web tests + check + build green.

**Deferred** — `keymap.toml` (P1.4c); `notifications` (P3); live reload of external `app.toml` edits (P1.3, reload-on-change).

## Scope [::state implemented]

**In scope** — the full appearance object, modelled in TOML + the domain wire:
`mode` · `palette_preset` · `density` · `accent_hue` · `glass_theme` (nested
blooms array). Including `glass_theme` avoids a split-home (basics-in-TOML /
glass-in-localStorage) that would defeat single-source-of-truth.

**Deferred**
- `keymap` → its own file `keymap.toml` (large, declarative) — **P1.4c**.
- `notifications` (`new_mail`/`sound`/`mutes`) — **P3** (✚-new, no migration).
- `updates.*`, `startup.*`, `storage.data_dir` — separate RFC rows.

## Decision: derived localStorage boot-cache (Option A) [::state in-progress]

The RFC says appearance is "moved from `localStorage` → TOML." Taken literally
(remove `localStorage` entirely), boot theme becomes async-only (settings are
fetched lazily today, only when the SettingsPanel opens) → **flash-of-wrong-theme
on every launch**. The standard fix that preserves "TOML = source of truth"
without the flash:

- **Boot**: apply theme from the `localStorage` cache immediately (no flash).
- **After the early settings fetch**: reconcile — **TOML wins** if it differs
  (re-apply + refresh cache). This is how an LLM/CLI edit to `app.toml` takes
  effect on next launch.
- **On user edit in the UI**: `PATCH /v1/settings` (writes `app.toml`) → on
  success, update the cache + apply.
- **One-time import**: if `[appearance]` is unset in TOML *and* `localStorage`
  has non-default values → write them into TOML once, then mirror back.

Net: `localStorage` is no longer "the config home" — the client-preferences
*snapshot* is retired as a config home; only a non-authoritative derived mirror
survives for boot speed. Ephemeral per-device geometry stays local (unchanged).
This matches the RFC's *spirit* (files are truth) without regressing boot UX.

## Schema [::state in-progress]

### Domain wire (`posthaste-domain`, camelCase via `AppSettings`)

```rust
struct Appearance {
    mode: Option<ThemeMode>,            // light | dark | system
    palette_preset: Option<PalettePresetId>, // neutral | paperInk | brutalist | glass | acid | marzipan | botanical
    density: Option<UiDensity>,         // compact | cozy | comfortable
    accent_hue: Option<u32>,            // 0–360
    glass_theme: Option<GlassTheme>,    // nested
}
struct GlassTheme { blooms: Vec<GlassBloom> }
struct GlassBloom { id: String, hue: u32, x: f64, y: f64, opacity: f64, radius: f64 }
```

Enums use `#[serde(rename_all = "camelCase")]` so `PaperInk` → `"paperInk"`
(matches the TS `PalettePresetId` set). The backend treats appearance as
**pass-through storage** (it does not interpret theme values); enums give
schema self-documentation + typo rejection at the parse boundary.

### TOML file (`posthaste-config`, snake_case — `app.toml`)

```toml
[appearance]
mode = "dark"
palette_preset = "neutral"
density = "compact"
accent_hue = 250

[[appearance.glass_theme.blooms]]
id = "bloom-1"
hue = 285
x = 20
y = 10
opacity = 0.35
radius = 45
```

Written via `write_managed_toml` (lossless: comments/unknown sections survive).
`APP_TOML_MANAGED_KEYS` gains `"appearance"`. `AppearanceToml` mirrors the domain
struct with `Option<_>` fields; a cleared `Option` removes its key.

## Vertical [::state in-progress]

1. `posthaste-domain` — `Appearance`/`GlassTheme`/`GlassBloom` + enums + `AppSettings.appearance` (openapi `ToSchema`).
2. `posthaste-runtime-contract` — `PatchAppSettingsMutation.appearance`.
3. `posthaste-authority-runtime` — `patch_app_settings` gains an `appearance` `AppSettingsFieldPatch` entry (audit name `"appearance"`).
4. `posthaste-config` — `AppearanceToml` + `AppToml::appearance` + `to_app_settings`/`from_app_settings` round-trip + `APP_TOML_MANAGED_KEYS`.
5. `posthaste-api` — `PatchSettingsRequest.appearance` + regenerate `openapi.json` + `schema.gen.ts`.
6. `apps/web` — `AppearancePane` reads/writes via `PATCH /v1/settings`; `ThemeProvider` boot-cache + reconcile (Option A); one-time import. (Flag sibling — both in `apps/web`.)
7. Tests — config lossless round-trip (glass array preserved), domain serde, openapi contract.
