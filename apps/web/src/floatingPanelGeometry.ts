import { FLOATING_PANEL_GRID, type ViewportSize } from './floatingPanelLayout'

export const FLOATING_PANEL_RAIL_RESISTANCE_DISTANCE = 12

export interface PanelOffset {
  x: number
  y: number
}

export interface PanelGeometry {
  width: number
  height: number
}

export interface ActiveRails {
  x: number | null
  y: number | null
}

export interface PanelRails {
  x: number[]
  y: number[]
}

export interface GuideColumn {
  left: number
  rail: number
  width: number
}

export interface GuideRow {
  height: number
  rail: number
  top: number
}

export interface GuideLayout {
  columns: GuideColumn[]
  rows: GuideRow[]
}

export interface RailDragState {
  lockedX?: number | null
  lockedY?: number | null
}

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

export function panelRailOffsets(
  panel: PanelGeometry,
  viewport: ViewportSize,
): PanelRails {
  const baseCenterX = viewport.width / 2
  const baseCenterY = FLOATING_PANEL_GRID.topOffset + panel.height / 2
  const horizontalCenters = [
    viewport.width / 6,
    viewport.width / 2,
    (viewport.width * 5) / 6,
  ].map((center) =>
    clamp(
      center,
      FLOATING_PANEL_GRID.screenMargin + panel.width / 2,
      viewport.width - FLOATING_PANEL_GRID.screenMargin - panel.width / 2,
    ),
  )
  const verticalCenters = [viewport.height / 4, (viewport.height * 3) / 4].map(
    (center) =>
      clamp(
        center,
        FLOATING_PANEL_GRID.screenMargin + panel.height / 2,
        viewport.height - FLOATING_PANEL_GRID.screenMargin - panel.height / 2,
      ),
  )

  return {
    x: uniqueRails(horizontalCenters.map((centerX) => centerX - baseCenterX)),
    y: uniqueRails(verticalCenters.map((centerY) => centerY - baseCenterY)),
  }
}

export function resistPanelOffset(
  offset: PanelOffset,
  panel: PanelGeometry,
  viewport: ViewportSize,
  drag: RailDragState,
): { activeRails: ActiveRails; offset: PanelOffset } {
  const clamped = clampPanelOffset(offset, panel, viewport)
  const rails = panelRailOffsets(panel, viewport)
  const x = resistRail(clamped.x, drag.lockedX, rails.x)
  const y = resistRail(clamped.y, drag.lockedY, rails.y)
  drag.lockedX = x.locked
  drag.lockedY = y.locked

  return {
    activeRails: { x: x.active, y: y.active },
    offset: { x: x.value, y: y.value },
  }
}

export function guideColumns(
  panel: PanelGeometry,
  viewport: ViewportSize,
): GuideColumn[] {
  const baseCenterX = viewport.width / 2

  return panelRailOffsets(panel, viewport).x.map((rail) => {
    const centerX = baseCenterX + rail
    return {
      left: centerX - panel.width / 2,
      rail,
      width: panel.width,
    }
  })
}

export function guideRows(
  panel: PanelGeometry,
  viewport: ViewportSize,
): GuideRow[] {
  const baseCenterY = FLOATING_PANEL_GRID.topOffset + panel.height / 2

  return panelRailOffsets(panel, viewport).y.map((rail) => {
    const centerY = baseCenterY + rail
    return {
      height: panel.height,
      rail,
      top: centerY - panel.height / 2,
    }
  })
}

function nearestRail(value: number, rails: number[]): number | null {
  let nearest: number | null = null
  let nearestDistance = Number.POSITIVE_INFINITY

  for (const rail of rails) {
    const distance = Math.abs(value - rail)
    if (distance < nearestDistance) {
      nearest = rail
      nearestDistance = distance
    }
  }

  return nearestDistance <= FLOATING_PANEL_RAIL_RESISTANCE_DISTANCE
    ? nearest
    : null
}

function resistRail(
  value: number,
  locked: number | null | undefined,
  rails: number[],
): { active: number | null; locked: number | null; value: number } {
  if (locked !== null && locked !== undefined) {
    if (Math.abs(value - locked) <= FLOATING_PANEL_RAIL_RESISTANCE_DISTANCE) {
      return { active: locked, locked, value: locked }
    }
  }

  const nearest = nearestRail(value, rails)
  if (nearest !== null) {
    return { active: nearest, locked: nearest, value: nearest }
  }

  return { active: null, locked: null, value }
}

export type ResizeHandle =
  | 'top'
  | 'right'
  | 'bottom'
  | 'left'
  | 'top-left'
  | 'top-right'
  | 'bottom-left'
  | 'bottom-right'

export interface PanelRect {
  left: number
  top: number
  width: number
  height: number
}

export interface ResizeConstraints {
  minWidth: number
  maxWidth: number
  minHeight: number
  maxHeight: number
}

export interface ResizeSnapLines {
  x: number[]
  y: number[]
}

export interface ActiveResizeLines {
  x: number[]
  y: number[]
}

export interface ResizeLocks {
  x: number | null
  y: number | null
}

export interface ResizeResult {
  size: PanelGeometry
  offset: PanelOffset
  activeLines: ActiveResizeLines
  locks: ResizeLocks
}

function resizeHandleAxes(handle: ResizeHandle): {
  horizontal: 'left' | 'right' | null
  vertical: 'top' | 'bottom' | null
} {
  return {
    horizontal: handle.includes('left')
      ? 'left'
      : handle.includes('right')
        ? 'right'
        : null,
    vertical: handle.includes('top')
      ? 'top'
      : handle.includes('bottom')
        ? 'bottom'
        : null,
  }
}

// Resize reuses the movement rails: the faint guide lines drawn while moving are
// the panel edges at each candidate slot, so a resized edge snaps to those same
// screen positions. Snap targets are frozen from the panel size at the start of
// the gesture, so the reference grid stays stable while the panel changes size.
export function resizeSnapLines(
  panel: PanelGeometry,
  viewport: ViewportSize,
): ResizeSnapLines {
  const columns = guideColumns(panel, viewport)
  const rows = guideRows(panel, viewport)
  return {
    x: uniqueRails(
      columns.flatMap((column) => [column.left, column.left + column.width]),
    ),
    y: uniqueRails(rows.flatMap((row) => [row.top, row.top + row.height])),
  }
}

export function resizePanelRect(input: {
  startRect: PanelRect
  handle: ResizeHandle
  dx: number
  dy: number
  constraints: ResizeConstraints
  viewport: ViewportSize
  snapLines: ResizeSnapLines
  locks: ResizeLocks
}): ResizeResult {
  const { startRect, handle, dx, dy, constraints, viewport, snapLines, locks } =
    input
  const { horizontal, vertical } = resizeHandleAxes(handle)
  const margin = FLOATING_PANEL_GRID.screenMargin
  const topAnchor = FLOATING_PANEL_GRID.topOffset

  let left = startRect.left
  let right = startRect.left + startRect.width
  let top = startRect.top
  let bottom = startRect.top + startRect.height

  const activeLines: ActiveResizeLines = { x: [], y: [] }
  const nextLocks: ResizeLocks = { x: null, y: null }

  if (horizontal === 'right') {
    const min = left + constraints.minWidth
    const max = Math.min(left + constraints.maxWidth, viewport.width - margin)
    right = clamp(startRect.left + startRect.width + dx, min, max)
    const snap = resistRail(right, locks.x, snapLines.x)
    right = clamp(snap.value, min, max)
    if (snap.active !== null && right === snap.active) {
      activeLines.x.push(snap.active)
      nextLocks.x = snap.locked
    }
  } else if (horizontal === 'left') {
    const min = Math.max(margin, right - constraints.maxWidth)
    const max = right - constraints.minWidth
    left = clamp(startRect.left + dx, min, max)
    const snap = resistRail(left, locks.x, snapLines.x)
    left = clamp(snap.value, min, max)
    if (snap.active !== null && left === snap.active) {
      activeLines.x.push(snap.active)
      nextLocks.x = snap.locked
    }
  }

  if (vertical === 'bottom') {
    const min = top + constraints.minHeight
    const max = Math.min(top + constraints.maxHeight, viewport.height - margin)
    bottom = clamp(startRect.top + startRect.height + dy, min, max)
    const snap = resistRail(bottom, locks.y, snapLines.y)
    bottom = clamp(snap.value, min, max)
    if (snap.active !== null && bottom === snap.active) {
      activeLines.y.push(snap.active)
      nextLocks.y = snap.locked
    }
  } else if (vertical === 'top') {
    const min = Math.max(margin, bottom - constraints.maxHeight)
    const max = bottom - constraints.minHeight
    top = clamp(startRect.top + dy, min, max)
    const snap = resistRail(top, locks.y, snapLines.y)
    top = clamp(snap.value, min, max)
    if (snap.active !== null && top === snap.active) {
      activeLines.y.push(snap.active)
      nextLocks.y = snap.locked
    }
  }

  const width = right - left
  const height = bottom - top
  return {
    size: { width, height },
    offset: { x: left - (viewport.width - width) / 2, y: top - topAnchor },
    activeLines,
    locks: nextLocks,
  }
}

function uniqueRails(values: number[]): number[] {
  const rails: number[] = []
  for (const value of values) {
    if (!rails.some((rail) => Math.abs(rail - value) < 1)) {
      rails.push(value)
    }
  }
  return rails
}

function clamp(value: number, min: number, max: number): number {
  // Guard against inverted bounds: the resize paths can derive a min above the
  // max at extreme viewport sizes, and silently collapsing to `max` could land
  // outside the screen margins.
  return Math.min(Math.max(value, Math.min(min, max)), Math.max(min, max))
}
