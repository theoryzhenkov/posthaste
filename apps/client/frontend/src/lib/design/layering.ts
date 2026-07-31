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
 * `src/app/assets/index.css` (`--z-base` … `--z-tooltip`) so Tailwind class-based surfaces
 * can reference them via `z-(--z-modal)` etc; `layering.test.ts` keeps the two in
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

/**
 * The WINDOW tier is a BAND, not a single value: peer windows (multiple compose
 * popups, floating panels) are raised WITHIN this band as they open or gain
 * focus so the most-recently-touched one is last-on-top. The band's ceiling
 * sits strictly BELOW OVERLAY, so a raised window can NEVER cover the command
 * palette, a dialog, or a toast — bring-to-front stays bounded.
 */
// Widened from the `as const` literals: these are arithmetic bounds, and the
// literal types would narrow every value derived from them.
const WINDOW_BAND_MIN: number = Z.WINDOW
const WINDOW_BAND_MAX: number = Z.OVERLAY - 100 // 2900 < OVERLAY (3000)

/**
 * A live WINDOW-tier panel's claim on a z within the band. Held by the panel
 * for as long as it is mounted; `z` is re-seated by the allocator, so read it
 * through the change callback rather than caching the number.
 */
export interface WindowSlot {
  z: number
}

/**
 * Every mounted WINDOW-tier panel, mapped to its change callback. The callback
 * means "your z moved and you did not ask for it" — the allocator re-seats live
 * slots — so it never fires for a move the holder itself requested.
 */
const windowSlots = new Map<WindowSlot, (z: number) => void>()

function assignWindowZ(slot: WindowSlot, z: number, notify: boolean): void {
  if (slot.z === z) {
    return
  }
  slot.z = z
  if (notify) {
    windowSlots.get(slot)?.(z)
  }
}

/**
 * Claim a slot for a newly mounted panel. It opens at the front of the band,
 * which is what "newest window is on top" means. Read the opening z off the
 * returned slot — the callback is for later, unrequested moves. Release it on
 * unmount.
 */
export function acquireWindowSlot(onChange: (z: number) => void): WindowSlot {
  const slot: WindowSlot = { z: WINDOW_BAND_MIN }
  windowSlots.set(slot, onChange)
  raise(slot, false)
  return slot
}

/** Drop a slot when its panel unmounts, freeing its position in the band. */
export function releaseWindowSlot(slot: WindowSlot): void {
  windowSlots.delete(slot)
}

/**
 * Raise `slot` above every other live panel.
 *
 * Ordering only ever needs to be RELATIVE among the handful of panels actually
 * open, so when the next value would run off the ceiling the band re-seats
 * every live slot densely from its floor, preserving order. The band therefore
 * holds only as many values as there are open panels and cannot exhaust.
 *
 * The previous allocator was a monotonic counter whose 900 slots were sized
 * against "realistic window counts" — but `bringToFront` fires on every
 * pointer-down, so it burned a slot per INTERACTION, not per window. Around 900
 * clicks (an afternoon, in an app that stays open for days) pinned every panel
 * to the ceiling, where they tied and bring-to-front silently stopped working:
 * the oldest-mounted panel won on DOM order until reload.
 */
export function raiseWindowSlot(slot: WindowSlot): void {
  raise(slot, true)
}

function raise(slot: WindowSlot, notifySelf: boolean): void {
  if (!windowSlots.has(slot)) {
    return
  }
  const others = [...windowSlots.keys()].filter((other) => other !== slot)
  const top = others.reduce(
    (max, other) => Math.max(max, other.z),
    WINDOW_BAND_MIN,
  )
  if (top < WINDOW_BAND_MAX) {
    assignWindowZ(slot, top + 1, notifySelf)
    return
  }
  others.sort((a, b) => a.z - b.z)
  for (const [index, seated] of [...others, slot].entries()) {
    // Clamp is unreachable below ~900 concurrent panels and exists only so the
    // band can never cross into OVERLAY, whatever the panel count.
    assignWindowZ(
      seated,
      Math.min(WINDOW_BAND_MIN + 1 + index, WINDOW_BAND_MAX),
      seated === slot ? notifySelf : true,
    )
  }
}
