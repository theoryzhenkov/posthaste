/**
 * Single source of truth for z-index layering across every GLOBAL floating
 * surface in the app.
 *
 * Before this module the app had no coherent scale — just scattered magic
 * numbers (`z-[70]`, `z-[80]`, `z-50`, `z-[2100]`, `z-[2200]`, `z-[2300]`, …).
 * That mis-tiered the surfaces: the compose popup (`z-[80]`) rendered ABOVE the
 * command palette (`z-[70]`) and the discard-draft dialog (`z-50`), so opening
 * either OVER a compose put it behind the window.
 *
 * The fix is one ASCENDING scale of named tiers with wide gaps. A new floating
 * surface picks the tier that matches what it IS; ordering is then correct by
 * construction. The numeric values are mirrored as CSS custom properties in
 * `src/index.css` (`--z-base` … `--z-tooltip`) so Tailwind class-based surfaces
 * can reference them via `z-(--z-modal)` etc; a drift test keeps the two in
 * sync.
 *
 * Tiers (low → high):
 *  - BASE     content / default flow
 *  - RAISED   in-flow local raises (sticky headers, resize handles, drag ghosts)
 *  - SURFACE  full-screen route takeover surfaces (SurfaceHost, settings pages)
 *  - POPOVER  dropdowns: select / popover / context-menu / inline menus
 *  - WINDOW   floating peer windows (compose popups, notifications, tag editor,
 *             shortcuts) — a *band*, see below
 *  - OVERLAY  the command palette (a global overlay, always above windows)
 *  - MODAL    alert-dialogs / confirmations (above everything you can be editing)
 *  - TOAST    sonner notifications
 *  - TOOLTIP  tooltips / onboarding coach-marks (top)
 */
export const Z = {
  BASE: 0,
  RAISED: 10,
  SURFACE: 100,
  POPOVER: 1000,
  WINDOW: 2000,
  OVERLAY: 3000,
  MODAL: 4000,
  TOAST: 5000,
  TOOLTIP: 6000,
} as const

export type LayerTier = keyof typeof Z

/**
 * The WINDOW tier is a BAND, not a single value: peer windows (multiple compose
 * popups, floating panels) are raised WITHIN this band as they open or gain
 * focus so the most-recently-touched one is last-on-top. The band's ceiling
 * sits strictly BELOW OVERLAY, so a raised window can NEVER cover the command
 * palette, a dialog, or a toast — bring-to-front stays bounded.
 *
 * 900 slots of headroom is ample for realistic window counts; on the (extreme)
 * saturation case the allocator clamps at the ceiling and newest windows tie at
 * the top rather than ever crossing into OVERLAY.
 */
export const WINDOW_BAND_MIN = Z.WINDOW
export const WINDOW_BAND_MAX = Z.OVERLAY - 100 // 2900 < OVERLAY (3000)

let windowRaiseCounter = 0

/**
 * Hand out the next z-index within the WINDOW band. Each open/focus of a peer
 * window calls this to jump to the front of its tier. The returned value is
 * always in `[WINDOW_BAND_MIN, WINDOW_BAND_MAX]` and therefore always `<
 * Z.OVERLAY`.
 */
export function nextWindowZIndex(): number {
  windowRaiseCounter += 1
  return Math.min(WINDOW_BAND_MIN + windowRaiseCounter, WINDOW_BAND_MAX)
}

/** Reset the bring-to-front counter. Test-only. */
export function resetWindowStacking(): void {
  windowRaiseCounter = 0
}
