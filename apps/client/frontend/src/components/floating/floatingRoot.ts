/**
 * The single container every floating panel portals into.
 *
 * Panels must escape their ancestors — a `backdrop-filter`/`transform`/`filter`
 * ancestor becomes the containing block for `fixed` and a backdrop root, which
 * would frost the ancestor's chrome instead of the page and resolve
 * `fixed inset-0` against the ancestor rather than the viewport. Portaling to
 * `document.body` already achieved that; this gives the panels one *owned*
 * mount point instead of interleaving them with everything else that lands on
 * body (Radix popovers, the toaster), so what sits in the floating layer is
 * explicit and inspectable.
 *
 * It deliberately carries no styles: a static, unpositioned div creates no
 * stacking context, so panels keep competing on z-index in the root stacking
 * context exactly as they did when they were body children. Ordering is the
 * WINDOW band's job (`lib/design/layering.ts`), not this element's.
 *
 * Created on first use rather than rendered by a provider, so a panel can mount
 * without depending on where that provider sits in the tree.
 */
const FLOATING_ROOT_ID = 'floating-root'

export function floatingRoot(): HTMLElement {
  const existing = document.getElementById(FLOATING_ROOT_ID)
  if (existing) {
    return existing
  }
  const root = document.createElement('div')
  root.id = FLOATING_ROOT_ID
  document.body.appendChild(root)
  return root
}
