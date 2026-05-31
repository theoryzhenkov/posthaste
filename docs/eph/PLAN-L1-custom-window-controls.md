---
scope: L1
type: PLAN
lifecycle: ephemeral
summary: "Replace native macOS traffic lights with app-drawn window controls (pastel, concealable, eventually cross-platform-unified)"
modified: 2026-05-31
reviewed: 2026-05-31
depends:
  - path: docs/L1-ui
  - path: docs/L2-ui-visual-reference
dependents: []
---

# PLAN: Custom window controls

## Why

Today every desktop window uses the **native macOS traffic lights** (via
`titleBarStyle: Overlay` + `hidden_title` + `traffic_light_position` in
`apps/desktop/src/lib.rs`; the web reserves the zone with `TrafficLightInset` /
`WindowTitlebar` in `apps/web/src/components/WindowChrome.tsx`). Three things we
want are impossible with OS-drawn lights:

1. **Recolor** the close/min/zoom controls to the app's pastel tokens (the OS
   owns the native red/yellow/green).
2. **Conceal** them — floating panes can't cover an OS-painted overlay, so the
   "semaphore" always sits above panes, which reads oddly.
3. **Pixel-exact placement** / a single chrome that is consistent across macOS,
   Windows, and Linux.

App-drawn controls give all three.

## Decision & status (2026-05-31)

**Deferred.** We chose to **keep native controls for now**. The tradeoff is real:
going custom means we lose the native **green-button hover menu** (macOS Sequoia
window tiling: "Fill / Move & Resize / tile left & right / Enter Full Screen"),
the native hover-reveal of the ×−+ glyphs, VoiceOver's knowledge that these are
window controls, and we take on ongoing upkeep to track macOS conventions. The
recolor + concealment are mostly aesthetic, so we did not consider them worth
that cost yet. Revisit when we want a fully bespoke, cross-platform-unified
chrome and are willing to own the maintenance.

The smaller, native-compatible complaint — the lights not being **centered** in
the toolbar — is fixable independently by tuning `traffic_light_position` and is
**not** part of this plan.

## Approach when we return

Prefer the **least-fragile path: keep the native window frame, hide only the
three native buttons, and draw custom controls** in the web layer. This preserves
native edge-resize, window shadow, rounded corners, and fullscreen — only the
three buttons become custom. Full `decorations: false` (frameless) is the
fallback, but it forfeits native edge-resize (would need `startResizeDragging`
hit areas) and complicates shadow/corners — avoid unless button-hiding proves
unworkable.

**Open technical question (research first, like the Cmd+W fix):** the exact API
to hide *only* the native traffic-light buttons while keeping `titleBarStyle:
Overlay`. Candidates, in order of preference:
- a Tauri v2 `WebviewWindow`/builder option for window-button visibility, if one exists;
- tao `WindowBuilderExtMacOS::with_titlebar_buttons_hidden` (or equivalent) surfaced through Tauri;
- dropping to objc2/cocoa: reach the `NSWindow` (`window.ns_window()`) and set
  `standardWindowButton(.closeButton|.miniaturizeButton|.zoomButton)?.isHidden = true`
  on the main thread after the window is built. Verify it survives fullscreen toggles.

## Implementation sketch (phased)

1. **Rust — hide native buttons.** In `build_window` (still `#[cfg(macos)]`),
   keep Overlay + hidden_title, drop `traffic_light_position`, and hide the three
   standard window buttons via the chosen API. Confirm native edge-resize/shadow/
   corners survive.
2. **Web — `WindowControls` component.** Replace `TrafficLightInset` (currently an
   empty draggable spacer) with three pastel buttons in the same 78px zone, macOS
   desktop only. Wire to `getCurrentWindow()`: close → `.close()`, minimize →
   `.minimize()`, zoom → `.toggleMaximize()` (decide vs `.setFullscreen()` for the
   green button). Hover reveals ×−−+ glyphs; colors from the app pastel tokens.
   Keep `data-tauri-drag-region` on the surrounding strip so the window still
   moves. Render the controls inside the main shell (ActionBar) and each surface
   window (`WindowChrome.WindowTitlebar`).
3. **Z-index (A3).** Because the controls are now web elements, ensure floating
   panes (`FloatingPanel`) stack above them so a pane can conceal the controls.
4. **Cross-platform unify (optional, later).** Extend custom controls to
   Windows/Linux (frameless + custom controls + `startResizeDragging` edge hit
   areas). Bigger lift; only if we want one chrome everywhere.

## Risks / gotchas

- Recoloring away from red/yellow/green hurts the learned "red = close"
  affordance; keep the hues, soften toward pastel.
- objc button-hiding must run on the main thread after window creation and must
  re-apply (or survive) fullscreen enter/exit.
- Losing the macOS tiling menu is the main UX regression — accept consciously.
- Keep `WINDOW_TRAFFIC_LIGHT_INSET` / `WINDOW_TITLEBAR_HEIGHT` (WindowChrome.tsx)
  as the single source of truth for control placement.

## Related

- `apps/desktop/src/lib.rs` — `build_window` (native title bar).
- `apps/web/src/components/WindowChrome.tsx` — `TrafficLightInset`, `WindowTitlebar`.
- `apps/web/src/components/ActionBar.tsx` — main-window chrome consumer.
- L2 §Floating Panel / §Action Bar; L1 "every desktop window inherits the same native chrome".
