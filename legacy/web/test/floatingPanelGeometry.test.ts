import { describe, expect, it } from 'bun:test'

import {
  clampPanelOffset,
  guideColumns,
  guideRows,
  panelRailOffsets,
  expandPanelToGrid,
  resistPanelOffset,
  resizeGridLines,
  resizePanelRect,
} from '../src/floatingPanelGeometry'
import { floatingPanelResizeConstraints } from '../src/floatingPanelLayout'

const viewport = { width: 1200, height: 800 }
const panel = { width: 600, height: 400 }

describe('floating panel geometry', () => {
  it('clamps panel offsets inside the viewport margins', () => {
    expect(clampPanelOffset({ x: 1000, y: -100 }, panel, viewport)).toEqual({
      x: 284,
      y: -38,
    })
  })

  it('derives movement rails from the panel size and viewport grid', () => {
    expect(panelRailOffsets(panel, viewport)).toEqual({
      x: [-284, 0, 284],
      y: [-38, 330],
    })
  })

  it('resists near rails until the pointer breaks out', () => {
    const drag = { lockedX: null, lockedY: null }

    const resisted = resistPanelOffset(
      { x: -280, y: -35 },
      panel,
      viewport,
      drag,
    )
    expect(resisted).toEqual({
      activeRails: { x: -284, y: -38 },
      offset: { x: -284, y: -38 },
    })
    expect(drag).toEqual({ lockedX: -284, lockedY: -38 })

    const released = resistPanelOffset(
      { x: -260, y: -10 },
      panel,
      viewport,
      drag,
    )
    expect(released).toEqual({
      activeRails: { x: null, y: null },
      offset: { x: -260, y: -10 },
    })
  })

  it('renders guide columns and rows around rail-aligned panel bounds', () => {
    expect(guideColumns(panel, viewport)).toEqual([
      { left: 16, rail: -284, width: 600 },
      { left: 300, rail: 0, width: 600 },
      { left: 584, rail: 284, width: 600 },
    ])
    expect(guideRows(panel, viewport)).toEqual([
      { top: 16, rail: -38, height: 400 },
      { top: 384, rail: 330, height: 400 },
    ])
  })

  it('keeps movement rails dynamic with the panel size', () => {
    // A smaller panel yields a different rail spacing, so rails restored after a
    // resize still fit the panel rather than its old footprint.
    expect(panelRailOffsets({ width: 300, height: 200 }, viewport)).toEqual({
      x: [-400, 0, 400],
      y: [46, 446],
    })
  })
})

// A viewport whose usable area divides evenly into the 12x8 grid, for clean
// integer grid lines: usable 1200x720 -> 100px columns, 90px rows.
const gridViewport = { width: 1232, height: 790 }
const gridConstraints = floatingPanelResizeConstraints(undefined, gridViewport)
const grid = resizeGridLines(gridViewport)
const gridStart = { left: 216, top: 144, width: 300, height: 180 }

describe('floating panel resize geometry', () => {
  it('exposes a static, viewport-anchored grid independent of any panel', () => {
    expect(grid).toEqual({
      x: [16, 116, 216, 316, 416, 516, 616, 716, 816, 916, 1016, 1116, 1216],
      y: [54, 144, 234, 324, 414, 504, 594, 684, 774],
    })
  })

  it('snaps a dragged edge to the nearest grid line, opposite edge fixed', () => {
    const result = resizePanelRect({
      startRect: gridStart,
      handle: 'right',
      dx: 95,
      dy: 0,
      constraints: gridConstraints,
      viewport: gridViewport,
      snapLines: grid,
      locks: { x: null, y: null },
    })
    expect(result.size).toEqual({ width: 400, height: 180 })
    expect(result.offset).toEqual({ x: -200, y: 90 })
    expect(result.activeLines).toEqual({ x: [616], y: [] })
  })

  it('clamps growth to the usable viewport (last grid line is the margin)', () => {
    const result = resizePanelRect({
      startRect: gridStart,
      handle: 'right',
      dx: 10000,
      dy: 0,
      constraints: gridConstraints,
      viewport: gridViewport,
      snapLines: grid,
      locks: { x: null, y: null },
    })
    expect(result.size.width).toBe(1000)
    expect(result.offset.x).toBe(100)
  })

  it('enforces the minimum width when no grid line is within reach', () => {
    const result = resizePanelRect({
      startRect: { left: 250, top: 144, width: 300, height: 180 },
      handle: 'right',
      dx: -10000,
      dy: 0,
      constraints: gridConstraints,
      viewport: gridViewport,
      snapLines: grid,
      locks: { x: null, y: null },
    })
    expect(result.size.width).toBe(gridConstraints.minWidth)
    expect(result.activeLines.x).toEqual([])
  })

  it('double-click grows a single edge out to the nearest grid line', () => {
    const result = expandPanelToGrid({
      rect: { left: 250, top: 144, width: 300, height: 180 },
      handle: 'left',
      both: false,
      grid,
      viewport: gridViewport,
    })
    // left 250 -> 216 (nearest line outward); right (550) unchanged.
    expect(result.size).toEqual({ width: 334, height: 180 })
    expect(result.offset).toEqual({ x: -233, y: 90 })
  })

  it('double-click a corner grows both grabbed edges to the grid', () => {
    const result = expandPanelToGrid({
      rect: { left: 250, top: 144, width: 300, height: 180 },
      handle: 'bottom-left',
      both: false,
      grid,
      viewport: gridViewport,
    })
    // left 250 -> 216, bottom 324 -> 414; top/right unchanged.
    expect(result.size).toEqual({ width: 334, height: 270 })
  })

  it('double-click on an edge already on a grid line grows one cell out', () => {
    const result = expandPanelToGrid({
      rect: { left: 216, top: 144, width: 200, height: 180 },
      handle: 'left',
      both: false,
      grid,
      viewport: gridViewport,
    })
    // left 216 sits on a line, so it grows to the next line out (116).
    expect(result.size.width).toBe(300)
  })

  it('double-click on an edge at the outer margin is a no-op for that axis', () => {
    const result = expandPanelToGrid({
      rect: { left: 16, top: 144, width: 200, height: 180 },
      handle: 'left',
      both: false,
      grid,
      viewport: gridViewport,
    })
    // left is already at the margin (first grid line); nothing to grow into.
    expect(result.size.width).toBe(200)
  })

  it('double-click with the modifier grows the opposite edges too', () => {
    const edge = expandPanelToGrid({
      rect: { left: 250, top: 144, width: 300, height: 180 },
      handle: 'left',
      both: true,
      grid,
      viewport: gridViewport,
    })
    // left 250 -> 216 AND right 550 -> 616 (horizontal both ways).
    expect(edge.size).toEqual({ width: 400, height: 180 })

    const corner = expandPanelToGrid({
      rect: { left: 250, top: 144, width: 300, height: 180 },
      handle: 'bottom-left',
      both: true,
      grid,
      viewport: gridViewport,
    })
    // all four edges grow: left->216, right->616, top 144->54, bottom 324->414.
    expect(corner.size).toEqual({ width: 400, height: 360 })
  })
})
