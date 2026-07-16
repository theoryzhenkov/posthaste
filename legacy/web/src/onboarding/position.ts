/**
 * Pure placement math for the onboarding tooltip card. Kept free of the DOM so
 * it can be unit-tested.
 */
export interface Rect {
  top: number
  left: number
  width: number
  height: number
}

export interface Size {
  width: number
  height: number
}

const GAP = 12

function clamp(value: number, min: number, max: number): number {
  if (max < min) return min
  return Math.min(Math.max(value, min), max)
}

/**
 * Place the card near the anchor, preferring below, then above, then centered
 * vertically beside it; always clamped within the viewport with a margin. With
 * no anchor the card is centered.
 */
export function computeCardPosition(input: {
  anchor: Rect | null
  viewport: Size
  card: Size
  margin?: number
}): { top: number; left: number } {
  const { anchor, viewport, card } = input
  const margin = input.margin ?? GAP

  const maxLeft = viewport.width - card.width - margin
  const maxTop = viewport.height - card.height - margin

  if (!anchor) {
    return {
      top: clamp((viewport.height - card.height) / 2, margin, maxTop),
      left: clamp((viewport.width - card.width) / 2, margin, maxLeft),
    }
  }

  const below = anchor.top + anchor.height + GAP
  const above = anchor.top - GAP - card.height
  const fitsBelow = below + card.height + margin <= viewport.height
  const top = fitsBelow ? below : above >= margin ? above : below

  // Align the card's left edge to the anchor, then clamp into view.
  const left = clamp(anchor.left, margin, maxLeft)
  return { top: clamp(top, margin, maxTop), left }
}
