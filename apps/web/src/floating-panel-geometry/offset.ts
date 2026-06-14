import { FLOATING_PANEL_GRID, type ViewportSize } from '../floatingPanelLayout'
import { clamp } from './math'
import type { PanelGeometry, PanelOffset } from './types'

export function isFiniteOffset(value: unknown): value is PanelOffset {
  return (
    typeof value === 'object' &&
    value !== null &&
    'x' in value &&
    'y' in value &&
    typeof value.x === 'number' &&
    typeof value.y === 'number' &&
    Number.isFinite(value.x) &&
    Number.isFinite(value.y)
  )
}

export function clampPanelOffset(
  offset: PanelOffset,
  panel: PanelGeometry,
  viewport: ViewportSize,
): PanelOffset {
  const baseLeft = (viewport.width - panel.width) / 2
  const minX = FLOATING_PANEL_GRID.screenMargin - baseLeft
  const maxX =
    viewport.width - FLOATING_PANEL_GRID.screenMargin - panel.width - baseLeft
  const minY = FLOATING_PANEL_GRID.screenMargin - FLOATING_PANEL_GRID.topOffset
  const maxY =
    viewport.height -
    FLOATING_PANEL_GRID.screenMargin -
    panel.height -
    FLOATING_PANEL_GRID.topOffset

  return {
    x: clamp(offset.x, Math.min(minX, maxX), Math.max(minX, maxX)),
    y: clamp(offset.y, Math.min(minY, maxY), Math.max(minY, maxY)),
  }
}
