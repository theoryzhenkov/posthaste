---
scope: L2
summary: "Separate the three concerns the client's theming system conflated — palette (which colours), material (opacity/blur/elevation), and compositing (which elements get their own layer). Material becomes a closed set of SURFACE ROLES supplied as typed data per theme; only the two floating tiers may carry backdrop-filter, which makes the glass-only notifications-panel occlusion structurally unrepresentable rather than merely fixed. Themes may no longer write selectors."
modified: 2026-07-30
reviewed: 2026-07-30
lifecycle: ephemeral
type: DESIGN
state: implemented
depends:
  - path: apps/client/frontend/src/lib/design/tokens/surfaces.ts
    note: "the typed material source of truth"
  - path: apps/client/frontend/src/lib/design/layering.ts
    section: "the Z scale this borrows its drift-test pattern from"
  - path: apps/client/frontend/src/app/assets/index.css
dependents: []
---

# DESIGN-L2-theming — surface roles and the compositing boundary

> **Status: DESIGN — IMPLEMENTED (2026-07-30).** All four migration steps landed
> as one commit each on top of `main`. Every claim about *structure* below is
> verified by a test (each mutation-checked); every claim about *appearance* is
> an inference — the change has not been rendered.

## 1. Problem

The client's theming lived in `src/app/assets/index.css` (~515 lines) plus
`src/lib/design/`, and it conflated three concerns that have to be separable.

**Palette** was four hand-maintained token blocks — `:root` (79 tokens), `.dark`
(56), `[data-palette-preset='glass']` (52), `.dark[data-palette-preset='glass']`
(51) — with nothing enforcing that a block defines what the others do. The
specificity is flat: `:root`, `.dark` and `[data-palette-preset='glass']` are all
(0,1,0), so source order alone decides. A token a theme forgets resolves from
whatever block happens to sit above it, silently.

**Material** was smuggled into the palette. `--panel: rgba(18,16,26,0.45)`
encodes translucency as a colour. `--background: transparent` made a *semantic*
token mean "nothing" in glass, so a component using `bg-background` to cover
something covered nothing.

**Compositing** was the real defect:

```css
[data-palette-preset='glass'] .bg-chrome,
[data-palette-preset='glass'] .bg-sidebar,
[data-palette-preset='glass'] .bg-panel,
[data-palette-preset='glass'] .bg-background,
[data-palette-preset='glass'] .bg-card,
[data-palette-preset='glass'] .bg-bg-elev {
  background-clip: padding-box;
  backdrop-filter: blur(44px) saturate(180%);
}
```

A palette reaching into utility classes and changing their rendering semantics.
Every `.bg-panel` in the app became a composited layer with its own stacking
context and backdrop root — but only under glass.

### 1.1 The live bug

The notifications panel (`NotificationsPanel` → `FloatingPanel`) was reported as
genuinely occluded by the message-display pane, in **both** glass presets and in
**neither** solid theme. `MessageDetail`'s root was `bg-panel` with a SECOND
nested `bg-panel` wrapping `MessageBody` → `EmailFrame`, a sandboxed `<iframe>`.
Under glass that is nested backdrop roots wrapping a separately-composited
iframe.

By plain CSS this cannot happen. The panel portals to `document.body`, is
`position: fixed`, and carries a z-index from the WINDOW band (~2001,
`layering.ts`); the pane is in normal flow with no z-index at all. A
glass-only failure in a rule that promotes panes to composited layers is a
compositing-order failure, and the rule above is the only candidate.

### 1.2 The fourth symptom

`FloatingPanel` did **not** use `.bg-panel`. It hand-rolled a brand gradient plus
`backdrop-blur-[24px]` — a second glass implementation at a different radius from
the theme's 44px, neither aware of the other. "Panels are standardised" held by
convention only. `OnboardingTour` was a third.

## 2. The design

### 2.1 Surface roles, not colours

A closed set of six roles, each rendered through exactly one utility:

| role | what it is | rendered by | z-tier |
|---|---|---|---|
| `chrome` | title bar / action bar / surface headers | `.surface-chrome` | BASE |
| `sidebar` | the mailbox rail and the settings rail | `.surface-sidebar` | BASE |
| `pane` | content panes (message list, detail, attachment host) | `.surface-pane` | BASE |
| `card` | a bounded content card inside a pane | `.surface-card` | BASE |
| `floating` | peer windows — compose popups, notifications, coach-marks | `.surface-floating` | WINDOW |
| `overlay` | the global overlay tier — the command palette | `.surface-overlay` | OVERLAY |

Each theme supplies a material per role: `--surface-<role>-fill`,
`--surface-<role>-border`, `--surface-<role>-shadow`, and — for the floating
tiers only — `--surface-<role>-blur`.

Themes declare material as **data**, in `src/lib/design/tokens/surfaces.ts`. No
theme may write a selector, and none can: the file emits no CSS. `applyRootTheme`
publishes the active theme's materials onto the root element as custom
properties, and the six `.surface-<role>` rules in `index.css` are the only
consumers. That is a stronger statement than "no theme may target `.bg-*`" — a
theme has no way to express a selector at all.

`bg-elev` did **not** become a role. All 16 of its uses take alpha modifiers
(`bg-bg-elev/45`), which a role class cannot express; it is an elevated *colour*,
not a surface. Leaf fills that establish no region — the ActionBar search chip,
the view-mode pill, the empty-state icon tile, the attachment preview iframe —
likewise keep their palette colour. A role is for a region, not for every fill.

### 2.2 `backdrop-filter` belongs to the floating tiers only

Only `floating` and `overlay` may composite. Content panes get translucency from
fill alpha instead.

This is enforced in the type, not in review. `ContentMaterial` has three fields;
`CompositedMaterial` adds `blur`. A content role has nowhere to put a
`backdrop-filter`, so a theme author cannot composite a pane even deliberately —
the alternative (a `blur` field typed to `'none'`) would still leave the field
there to be changed. Nested backdrop roots and content-pane stacking contexts
are therefore unrepresentable.

It is also cheaper: blur runs on the two or three floating elements that are open
rather than on every pane on screen.

### 2.3 One typed contract, drift-tested

The repo already had the right pattern — `layering.ts` defines `Z`, mirrors it to
`--z-*`, and `layering.test.ts` fails on drift. Surfaces go one better: the CSS
mirror does not exist, because the values are written at runtime from the typed
record. What remains testable is the *consumer* side, and
`src/lib/design/tokens/surfaces.test.ts` asserts it exhaustively:

| assertion | what it kills |
|---|---|
| every theme publishes the same token set, no empty values | a half-finished theme |
| only floating tiers publish a `blur` | material leaking onto content roles |
| `index.css` references exactly the tokens the themes publish | a stale or invented token |
| every role has one rule that paints fill/border/shadow | a role with no renderer |
| no CSS rule outside `.surface-floating`/`.surface-overlay` declares `backdrop-filter` | the original defect, reinstated |
| no rule keyed on a palette attribute names a class | a theme rewriting a utility |
| no `className` puts `backdrop-blur`/`-filter` beside a content-role class | the same defect, from the component side |

Completeness of the themes themselves is the compiler's job:
`Record<SurfaceThemeKey, SurfaceTheme>` over `${ThemeStyle}-${ResolvedThemeMode}`
makes a new structural style force four entries and a missing facet a type error.

The palette is still hand-written CSS, so it gets the same guarantee the
expensive way — `src/lib/design/palette.test.ts` requires every theme block to
declare every mode-scoped token itself, with two escape hatches that cost a
written justification (structural tokens, which must live in `:root` alone; and
an `EXEMPT_TOKENS` table currently holding `--radius` and the two runtime-owned
surface-hue knobs). A vacuity guard stops the file passing by parsing nothing.

### 2.4 One glass implementation

`FloatingPanel` takes the `floating` role (`overlay` for the command-palette
tier). Its gradient and its 24px blur are gone; both come from the theme.
`OnboardingTour`'s hand-rolled card takes `floating` too. `ResizeCellBadge` drops
its `backdrop-blur-sm`: it lives inside the floating panel, so it was a backdrop
root nested in a backdrop root.

The `overlay` role is not a copy of `floating`. It sits on `--popover` rather
than `--panel` — the most opaque palette surface — because a command palette is
read while everything behind it is still moving.

## 3. Migration (one commit per step, each independently shippable)

| step | commit | contents |
|---|---|---|
| 1 | `feat(client): introduce surface roles and per-theme material tokens` | `surfaces.ts` + the six utilities + `applyRootTheme` publishing them; `FloatingPanel` adopts `floating`/`overlay`. The glass compositing rule untouched. |
| 2 | `fix(client): take backdrop-filter off the content panes` | the rule deleted; region surfaces migrated to role utilities; glass fills gain alpha; the nested `bg-panel`s removed; the three compositing invariants added. **The occlusion dies here.** |
| 3 | `test(client): make an incomplete palette block fail loudly` | `palette.test.ts`; the three tokens both glass blocks were silently inheriting are declared. |
| 4 | `fix(client): give glass a --background that actually covers` | `--background: transparent` retired in both glass presets; a no-token-means-nothing assertion. |

Step 3 was resequenced. The plan of record had it *generating* the theme blocks
from the typed source; step 1 already does better than that for surfaces (there
is no CSS to generate — the values are applied at runtime), so step 3's remaining
value was the completeness gate on the half that is still CSS, the palette.

## 4. What the diagnosis got wrong

- **"A token missing from a block silently inherits from `:root`, which is the
  LIGHT theme."** True as a hazard, not as a present fact. `.dark` sits between
  `:root` and the glass blocks in source order and defines every colour token, so
  a glass-dark omission today lands on `.dark` — the correct *mode*, the wrong
  *style*. The realised bug was style-shaped, not mode-shaped:
  `--list-selection-muted` and its foreground were inheriting the solid palette's
  **opaque** oklch greys into a translucent theme, so an unfocused list selection
  in glass painted an opaque slab beside a translucent focused one. The hazard as
  stated becomes real the moment a third style ships without a `.dark` twin,
  which is why the gate requires each block to be self-complete.
- **`--destructive`** was likewise undeclared in both glass blocks. It resolved
  to the right value by luck of ordering; it is now written down.
- **The count of things needing a role was over-estimated.** `bg-bg-elev` (16
  uses) and several `bg-panel` uses are colour, not surface — see §2.1.

## 5. Honest limits

- **No claim here about appearance has been observed.** Nobody rendered the app.
  The glass fills were raised to compensate for the blur that panes lost
  (chrome .60 → .72, sidebar .55 → .66, pane .70 → .82; dark .55/.40/.45 →
  .70/.58/.64) — those numbers are reasoned from "alpha must now do the work
  blur was doing", not tuned against a screen. Expect them to need a pass.
- **`--accent`, `--secondary`, `--muted`** are opaque ramps in the solid themes
  and low-alpha washes in glass. That is a contract difference of the same family
  as `--background`'s, left alone because those tokens are hover/selection washes
  by definition and are never used to cover anything.
- **`GlassMeshEditor`** keeps a `backdrop-blur-[2px]` on a decorative overlay
  inside the settings mesh preview. It is a content-flow backdrop root and
  therefore against the spirit of §2.2, but nothing is portaled into that
  subtree, and it carries no role class so the invariant test does not fire on
  it. Left as a known, bounded exception.
- **`bg-panel` / `bg-card` still exist** as palette utilities for leaf fills. A
  future pass could rename them so the distinction between "a surface" and "a
  colour that happens to be the panel colour" is visible at the call site.

## 6. Is the occlusion fixed, or structurally impossible?

Structurally impossible **for the mechanism identified**, and regression-proofed:

1. The rule that composited content panes is deleted.
2. No theme can reinstate it — themes emit no selectors.
3. No role can acquire a blur — content materials have no `blur` field.
4. No stylesheet rule outside the two floating tiers may declare
   `backdrop-filter`, and no component may put a blur beside a content-role
   class. Both are tested, and both tests were verified by mutation: reinstating
   the deleted rule fails two of them, pasting `backdrop-blur-sm` onto
   `MessageDetail` fails the third.
5. `MessageDetail`'s nested `bg-panel` is gone, so even the pre-condition (a pane
   inside a pane wrapping a composited iframe) no longer exists.

The residual uncertainty is diagnostic, not structural: nobody rendered the bug,
so "a composited content pane occluded a portaled fixed panel" remains an
inference from *glass-only* + *no plain-CSS explanation* + *the only glass-only
rendering rule in the file*. If the occlusion survives this change, the cause was
never compositing, and the next suspects are the portal target and the WINDOW
band allocator in `layering.ts` — neither of which this touches.

## 7. Code anchors

- Typed material source of truth: `apps/client/frontend/src/lib/design/tokens/surfaces.ts`
- Runtime application: `apps/client/frontend/src/lib/design/applyRootTheme.ts`
- The six role utilities + the palette blocks: `apps/client/frontend/src/app/assets/index.css`
- Surface contract + compositing invariants: `apps/client/frontend/src/lib/design/tokens/surfaces.test.ts`
- Palette completeness gate: `apps/client/frontend/src/lib/design/palette.test.ts`
- Layering scale (the pattern this borrows): `apps/client/frontend/src/lib/design/layering.ts`
- The one floating-surface component: `apps/client/frontend/src/components/floating/FloatingPanel.tsx`
