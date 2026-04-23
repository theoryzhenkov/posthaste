---
scope: L0
summary: "PostHaste brand identity, visual direction, palette, typography, and signal colors"
modified: 2026-04-23
reviewed: 2026-04-23
depends:
  - path: README
dependents:
  - path: docs/L0-ui
  - path: docs/L1-ui
  - path: docs/L2-ui-visual-reference
---

# Branding -- L0

## Name

**PostHaste** is the product name. UI copy may use `Posthaste` only when matching the exported handoff's title casing in window titles, modal copy, or prototype-sourced labels.

The name comes from the English expression "post-haste": urgent mail, forward motion, and decisive handling. The interface should feel fast, exact, and mail-native.

## Brand Direction

The exported reference in `.claude-design/Posthaste.standalone.bundled.html` supersedes the older light-first brand direction. The default product surface is now a dark neutral, mail-client power UI with warm postal accents and clear signal separation.

The visual language is dense, quiet, and precise. It uses compact rows, thin dividers, mono metadata, colored stamps, and restrained icon color. It should not become a large-card dashboard, a marketing surface, or a one-color theme.

## Source Of Truth

The reference implementation is the standalone handoff HTML and its unpacked JSX under `.claude-design/handoff/posthaste/project/src/`.

The default handoff state is:

- Theme: `dark`
- Preset: `neutral`
- Density: `standard`
- Layout: `3`
- Advanced controls: visible

When code and this document disagree, this document is the implementation target until the user reviews and changes it.

## Typography

The UI uses a locked type ramp. Components must not introduce new font sizes unless this spec is updated.

| Token | Size | Use |
|---|---:|---|
| `meta` | `11px` | Mono caps, column headers, kbd hints, counts, compact timestamps |
| `ui` | `12px` | Toolbar chips, sidebar account labels, metadata, compact controls |
| `body` | `13px` | Sidebar rows, message rows, reader body, compose fields |
| `emph` | `14px` | Account names, emphasized rows, secondary headings |
| `head` | `17px` | Reader subject, modal title, settings title |
| `sect` | `22px` | Rare section or about headings only |

Font families:

- Sans: `Geist`, then `-apple-system`, `BlinkMacSystemFont`, `Segoe UI`, `sans-serif`
- Mono: `Geist Mono`, then `ui-monospace`, `SF Mono`, `Menlo`, `monospace`
- Display: `Fraunces`, then `Georgia`, `serif`

Geist is the default everywhere. Geist Mono is required for metadata, counters, keyboard hints, dates, account abbreviations, section labels, column headers, and technical values. Fraunces is available only for rare display moments; it is not used for normal pane headings.

## Icon System

Icons use a locked grid:

| Token | Size | Stroke |
|---|---:|---:|
| `xs` | `12px` | `1.1` |
| `sm` | `14px` | `1.25` |
| `md` | `16px` | `1.4` |
| `lg` | `20px` | `1.6` |

Toolbar, sidebar, table headers, row markers, and modal list rows usually use `sm`. Kbd/control adornments may use `xs`. Modal headers and large stamps may use `md` or `lg`.

## Shape System

Radii are locked:

| Token | Radius | Use |
|---|---:|---|
| `xs` | `3px` | Kbd pills, resize hovers, tiny edit buttons |
| `sm` | `4px` | Account stamps, attachment type tiles |
| `md` | `6px` | Toolbar chips, search, attachment rows |
| `lg` | `10px` | Entity cards, modal icon tiles |
| `xl` | `14px` | Command palette shell and large body sheet |

The main app shell is not a card in embedded mode. Cards are reserved for concrete entities or modal surfaces.

## Neutral Dark Palette

The neutral dark palette is the default production target.

| Token | Value | Use |
|---|---|---|
| `bg` | `oklch(0.22 0.008 60)` | Root background |
| `bgElev` | `oklch(0.26 0.008 60)` | Raised rows, controls, attachment strip |
| `bgSidebar` | `oklch(0.195 0.008 55)` | Sidebar pane |
| `bgList` | `oklch(0.235 0.008 60)` | Message list pane |
| `bgListAlt` | `oklch(0.255 0.008 60)` | Zebra row tint |
| `bgReader` | `oklch(0.27 0.008 60)` | Reader pane |
| `bgTitlebar` | `oklch(0.185 0.008 55)` | Toolbar and table header |
| `border` | `oklch(0.32 0.008 60)` | Primary pane border |
| `borderSoft` | `oklch(0.28 0.008 60)` | Row, modal, and control separators |
| `borderStrong` | `oklch(0.4 0.01 60)` | Table header bottom rule |
| `fg` | `oklch(0.94 0.005 80)` | Primary text |
| `fgMuted` | `oklch(0.72 0.008 70)` | Secondary text |
| `fgSubtle` | `oklch(0.60 0.008 70)` | Low-priority metadata |
| `fgFaint` | `oklch(0.48 0.008 60)` | Section labels, disabled labels |
| `selBg` | `oklch(0.34 0.06 250)` | Selected sidebar/list row |
| `selFg` | `oklch(0.98 0.01 250)` | Selected row text/icon |
| `focusRing` | `oklch(0.68 0.15 250)` | Keyboard focus ring |
| `hoverBg` | `oklch(0.29 0.008 60)` | Hover rows and icon buttons |
| `shadow` | `0 2px 6px rgba(0,0,0,0.3), 0 8px 32px rgba(0,0,0,0.25)` | Default elevation |

## Accent And Signal Colors

The interface must preserve three separate signal families:

- Brand/flag uses coral.
- Unread uses blue.
- Selection uses slate-blue.

| Token | Value | Use |
|---|---|---|
| `accent.coral` | `oklch(0.68 0.17 45)` | Postmark brand, primary action, flag, active resize line |
| `accent.coralSoft` | `oklch(0.92 0.055 50)` | Active nav/control fill, coral chip background |
| `accent.coralDeep` | `oklch(0.52 0.18 38)` | Text/icon on coral-soft fill |
| `accent.sage` | `oklch(0.68 0.08 145)` | Newsletters, sync-ok dot, positive/quiet status |
| `accent.sageSoft` | `oklch(0.93 0.03 145)` | Sage wash |
| `accent.blue` | `oklch(0.65 0.13 245)` | Unread dot and unread counters |
| `accent.amber` | `oklch(0.78 0.13 78)` | Read-later/snooze |
| `accent.violet` | `oklch(0.65 0.13 295)` | Bills/tag category |
| `accent.rose` | `oklch(0.70 0.15 12)` | PDF/file tile and danger-adjacent accent |
| `signal.unread` | `oklch(0.65 0.13 245)` | All unread dots and unread pills |
| `signal.flag` | `oklch(0.68 0.17 45)` | Flag icon and flagged quick filter |

Unread counters next to mailbox names use `signal.unread` as the filled pill background only when the counter is in an account header. Sidebar row counters use mono `meta` text in `fgFaint` unless the row is selected, in which case they use `selFg`.

## Reference Sample Colors

Account stamps use account-provided color values:

| Account | Stamp | Color |
|---|---|---|
| Gmail | `G` | `oklch(0.72 0.15 25)` |
| Work | `W` | `oklch(0.68 0.12 240)` |
| University | `U` | `oklch(0.68 0.10 145)` |

Tag chips use tag-provided color values:

| Tag | Color |
|---|---|
| `work` | `oklch(0.68 0.12 240)` |
| `personal` | `oklch(0.68 0.10 145)` |
| `billing` | `oklch(0.65 0.13 295)` |
| `follow-up` | `oklch(0.68 0.17 45)` |
| `read-later` | `oklch(0.78 0.13 78)` |

## Product Character

The UI should read as a serious mail workstation: compact, gridded, high information density, and visually calm. It should use color for semantic information, not decoration.

Avoid:

- Gradient or mesh backgrounds in the default shell.
- Large nested cards in the main app view.
- Coral selection backgrounds for list/sidebar selection.
- Purple or blue-purple as the dominant brand color.
- Hidden text labels for core mail actions where the handoff shows labels.
