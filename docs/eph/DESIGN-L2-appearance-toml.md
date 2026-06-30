---
scope: L2
summary: "Migrate UI appearance prefs (theme mode/palette/density/accent/glass) from the renderer's localStorage snapshot into TOML as the single source of truth, with a derived localStorage boot-cache to avoid FOUC. Implements P1.4b of the configuration-surface RFC. Also home to the sibling per-entity presentation overlays in app.toml (tags)."
modified: 2026-06-30
reviewed: 2026-06-30
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
`mode` · `theme` (free-form id) · `density` · per-mode `light`/`dark`
(`accent_hue` · `surface_hue` · `tokens`) · `glass_theme` (nested
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
    theme: Option<String>,              // free-form id; built-ins "neutral" ("Classic") / "glass"
    density: Option<UiDensity>,         // compact | cozy | comfortable
    light: Option<ThemeColors>,         // per-mode color overrides
    dark: Option<ThemeColors>,
    glass_theme: Option<GlassTheme>,    // nested
}
struct ThemeColors {
    accent_hue: Option<u32>,            // 0–360
    surface_hue: Option<u32>,           // base/"main" color hue, today fixed grey
    tokens: BTreeMap<String, String>,   // arbitrary CSS custom-property overrides
}
struct GlassTheme { blooms: Vec<GlassBloom> }
struct GlassBloom { id: String, hue: u32, x: f64, y: f64, opacity: f64, radius: f64 }
```

The backend treats appearance as **pass-through storage** (it does not interpret
theme values). `theme` is a **free-form string** (not an enum) so user-created
themes are expressible without a schema change — the built-ins are `"neutral"`
(displayed "Classic") and `"glass"`. Color customization is **per-mode**
(`light`/`dark`). `ThemeColors.tokens` is the **foundation for user-supplied
themes / imported CSS**: a future loader parses a `.css` file's custom-property
declarations into this map; the renderer applies recognized tokens and ignores
the rest. (Earlier shape: a `PalettePresetId` enum + single top-level
`accent_hue` — both retired; see back-compat below.)

### TOML file (`posthaste-config`, snake_case — `app.toml`)

```toml
[appearance]
mode = "dark"
theme = "glass"
density = "compact"

[appearance.light]
accent_hue = 210
surface_hue = 40

[appearance.dark]
accent_hue = 250
surface_hue = 260

[[appearance.glass_theme.blooms]]
id = "bloom-1"
hue = 285
x = 20
y = 10
opacity = 0.35
radius = 45
```

Written via `write_managed_toml` (lossless: comments/unknown sections survive).
`APP_TOML_MANAGED_KEYS` includes `"appearance"`. `AppearanceToml` mirrors the
domain struct with `Option<_>` fields; a cleared `Option` removes its key.

**Back-compat** (files shipped since .30): an older `palette_preset` reads into
`theme` (serde alias), and a legacy top-level `accent_hue` seeds both `light` and
`dark` when neither is set. Neither legacy key is written back — the next save
re-serializes the per-mode shape, migrating the file in place.

**Transitional render bridge**: the design layer is still single-accent +
`palettePreset`, so `wireMapping` currently collapses the per-mode accent to one
and reads `theme` as the palette id. The design-layer revamp (per-mode colors +
`surface_hue` + `tokens`, owned by the sibling) makes it lossless.

## Vertical [::state in-progress]

1. `posthaste-domain` — `Appearance`/`GlassTheme`/`GlassBloom` + enums + `AppSettings.appearance` (openapi `ToSchema`).
2. `posthaste-runtime-contract` — `PatchAppSettingsMutation.appearance`.
3. `posthaste-authority-runtime` — `patch_app_settings` gains an `appearance` `AppSettingsFieldPatch` entry (audit name `"appearance"`).
4. `posthaste-config` — `AppearanceToml` + `AppToml::appearance` + `to_app_settings`/`from_app_settings` round-trip + `APP_TOML_MANAGED_KEYS`.
5. `posthaste-api` — `PatchSettingsRequest.appearance` + regenerate `openapi.json` + `schema.gen.ts`.
6. `apps/web` — `AppearancePane` reads/writes via `PATCH /v1/settings`; `ThemeProvider` boot-cache + reconcile (Option A); one-time import. (Flag sibling — both in `apps/web`.)
7. Tests — config lossless round-trip (glass array preserved), domain serde, openapi contract.

## Tags appearance overlay [::state implemented]

A sibling presentation overlay that lives in the same `app.toml` and rides the
same TOML-is-truth + `PATCH /v1/settings` + `write_managed_toml` machinery as
`[appearance]` (and the older `[[mailbox_colors]]`). Documented here because the
new code points its `@spec` at this design.

**Model.** Tags are *keyword-derived* — a tag exists because some message carries
a non-`$` keyword; there is no tag entity. Appearance is therefore an **overlay
keyed by tag name**, **global** (a `work` tag looks the same across accounts),
and pure presentation. Every field is optional; an absent field falls back to the
renderer's name-derived default (a subtle tint of the name accent + the generic
tag icon), so unconfigured tags still render as colored chips.

```rust
// posthaste-domain, camelCase via AppSettings.tags: Vec<TagAppearance>
struct TagAppearance {
    name: String,            // the tag (keyword) this override applies to
    fg: Option<String>,      // foreground/text color (CSS color string)
    bg: Option<String>,      // background color (CSS color string)
    icon: Option<String>,    // lucide icon name, e.g. "briefcase"
}
```

```toml
# app.toml — an [[tags]] array table; absent optional fields are omitted.
[[tags]]
name = "work"
fg = "#1f2937"
bg = "#dbeafe"
icon = "briefcase"
```

**Vertical** (mirrors the appearance one): `posthaste-domain` `TagAppearance` +
`AppSettings.tags` (openapi `ToSchema`); `posthaste-runtime-contract`
`PatchAppSettingsMutation.tags`; `posthaste-authority-runtime` an `AppSettingsFieldPatch`
entry (audit name `"tags"`); `posthaste-config` `TagAppearanceToml` + round-trip +
**`APP_TOML_MANAGED_KEYS` gains `"tags"`** (the allowlist silently drops unlisted
keys on write); `posthaste-api` `PatchSettingsRequest.tags` + regenerated
`openapi.json`/`schema.gen.ts`. Tests: `patch_settings_persists_tag_appearance_to_app_toml`.

**Render** (`apps/web`). `components/tags/model.tsx` owns the curated color
swatches + lucide icon set and `resolveTagStyle(name, override)`; `<TagChip>` +
`useTagAppearanceLookup` apply it (reactive to the settings query) in the tag
editor and the message-list tags column. The **Tags** settings pane edits the
overlay via the optimistic `useTagAppearanceMutation` (same shape as
`useMailboxColorMutation`). Color is a **curated fg/bg palette** and icon a
**curated lucide grid** for v1; the stored model (free-form color strings +
icon name) already allows arbitrary values later without a schema change.
