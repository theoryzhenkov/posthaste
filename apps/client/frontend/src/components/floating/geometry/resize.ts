import { FLOATING_PANEL_GRID, type ViewportSize } from './layout'
import { clamp } from './math'
import { resistRail } from './rails'
import type {
  ActiveResizeLines,
  PanelGeometry,
  PanelOffset,
  PanelRect,
  ResizeConstraints,
  ResizeHandle,
  ResizeLocks,
  ResizeResult,
  ResizeSnapLines,
} from './types'

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

// Resize snaps to a STATIC, viewport-anchored grid — the same 12x8 grid the size
// presets are built from — independent of the panel being dragged. Every
// floating window therefore snaps to the same standard, symmetric dimensions.
// (Movement rails, by contrast, stay sized to the current panel; see
// `panelRailOffsets`.)
export function resizeGridLines(viewport: ViewportSize): ResizeSnapLines {
  const usableWidth = Math.max(
    0,
    viewport.width - FLOATING_PANEL_GRID.screenMargin * 2,
  )
  const usableHeight = Math.max(
    0,
    viewport.height -
      FLOATING_PANEL_GRID.topOffset -
      FLOATING_PANEL_GRID.screenMargin,
  )
  const x: number[] = []
  for (let column = 0; column <= FLOATING_PANEL_GRID.columns; column += 1) {
    x.push(
      FLOATING_PANEL_GRID.screenMargin +
        (usableWidth * column) / FLOATING_PANEL_GRID.columns,
    )
  }
  const y: number[] = []
  for (let row = 0; row <= FLOATING_PANEL_GRID.rows; row += 1) {
    y.push(
      FLOATING_PANEL_GRID.topOffset +
        (usableHeight * row) / FLOATING_PANEL_GRID.rows,
    )
  }
  return { x, y }
}

// One grid cell, used to report the panel size in whole columns/rows.
export function resizeGridCell(viewport: ViewportSize): PanelGeometry {
  return {
    width:
      Math.max(0, viewport.width - FLOATING_PANEL_GRID.screenMargin * 2) /
      FLOATING_PANEL_GRID.columns,
    height:
      Math.max(
        0,
        viewport.height -
          FLOATING_PANEL_GRID.topOffset -
          FLOATING_PANEL_GRID.screenMargin,
      ) / FLOATING_PANEL_GRID.rows,
  }
}

function nearestGridLine(
  lines: number[],
  value: number,
  direction: 'down' | 'up',
): number | null {
  const epsilon = 0.5
  let best: number | null = null
  for (const line of lines) {
    if (direction === 'down') {
      if (line < value - epsilon && (best === null || line > best)) {
        best = line
      }
    } else if (line > value + epsilon && (best === null || line < best)) {
      best = line
    }
  }
  return best
}

// Double-click a handle to grow the panel out to the nearest grid lines. The
// grabbed edge(s) expand outward to the nearest line; with `both`, the opposite
// edge on each affected axis also expands — so an edge grows on both sides of
// its axis and a corner grows on all four.
export function expandPanelToGrid(input: {
  rect: PanelRect
  handle: ResizeHandle
  both: boolean
  grid: ResizeSnapLines
  viewport: ViewportSize
}): { size: PanelGeometry; offset: PanelOffset } {
  const { rect, handle, both, grid, viewport } = input
  const { horizontal, vertical } = resizeHandleAxes(handle)
  let left = rect.left
  let right = rect.left + rect.width
  let top = rect.top
  let bottom = rect.top + rect.height

  if (horizontal !== null) {
    if (horizontal === 'left' || both) {
      left = nearestGridLine(grid.x, left, 'down') ?? left
    }
    if (horizontal === 'right' || both) {
      right = nearestGridLine(grid.x, right, 'up') ?? right
    }
  }
  if (vertical !== null) {
    if (vertical === 'top' || both) {
      top = nearestGridLine(grid.y, top, 'down') ?? top
    }
    if (vertical === 'bottom' || both) {
      bottom = nearestGridLine(grid.y, bottom, 'up') ?? bottom
    }
  }

  const width = right - left
  const height = bottom - top
  return {
    size: { width, height },
    offset: {
      x: left - (viewport.width - width) / 2,
      y: top - FLOATING_PANEL_GRID.topOffset,
    },
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
