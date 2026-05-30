import { describe, expect, it } from 'bun:test'

import {
  clampPanelOffset,
  guideColumns,
  guideRows,
  panelRailOffsets,
  resistPanelOffset,
  resizePanelRect,
  resizeSnapLines,
} from '../src/floatingPanelGeometry'
import { floatingPanelResizeConstraints } from '../src/floatingPanelLayout'

const viewport = { width: 1200, height: 800 }
const panel = { width: 600, height: 400 }
const constraints = floatingPanelResizeConstraints(undefined, viewport)
const startRect = { left: 300, top: 16, width: 600, height: 400 }

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

describe('floating panel resize geometry', () => {
  it('derives resize snap lines from the movement guide edges', () => {
    expect(resizeSnapLines(panel, viewport)).toEqual({
      x: [16, 616, 300, 900, 584, 1184],
      y: [16, 416, 384, 784],
    })
  })

  it('snaps a dragged edge to the nearest rail and keeps the opposite edge fixed', () => {
    const snapLines = resizeSnapLines(panel, viewport)
    const result = resizePanelRect({
      startRect,
      handle: 'left',
      dx: 4,
      dy: 0,
      constraints,
      viewport,
      snapLines,
      locks: { x: null, y: null },
    })
    expect(result).toEqual({
      size: { width: 600, height: 400 },
      offset: { x: 0, y: -38 },
      activeLines: { x: [300], y: [] },
      locks: { x: 300, y: null },
    })
  })

  it('snaps a dragged bottom edge to a horizontal rail', () => {
    const snapLines = resizeSnapLines(panel, viewport)
    const result = resizePanelRect({
      startRect,
      handle: 'bottom',
      dx: 0,
      dy: 5,
      constraints,
      viewport,
      snapLines,
      locks: { x: null, y: null },
    })
    expect(result.size).toEqual({ width: 600, height: 400 })
    expect(result.activeLines).toEqual({ x: [], y: [416] })
  })

  it('clamps growth to the usable viewport', () => {
    const snapLines = resizeSnapLines(panel, viewport)
    const result = resizePanelRect({
      startRect,
      handle: 'right',
      dx: 10000,
      dy: 0,
      constraints,
      viewport,
      snapLines,
      locks: { x: null, y: null },
    })
    expect(result.size).toEqual({ width: 884, height: 400 })
    expect(result.offset).toEqual({ x: 142, y: -38 })
  })

  it('enforces the minimum width even when a rail is within reach', () => {
    const snapLines = resizeSnapLines(panel, viewport)
    const result = resizePanelRect({
      startRect,
      handle: 'right',
      dx: -10000,
      dy: 0,
      constraints,
      viewport,
      snapLines,
      locks: { x: null, y: null },
    })
    expect(result.size.width).toBe(constraints.minWidth)
    expect(result.activeLines.x).toEqual([])
  })

  it('drives both axes and both offset components from a corner handle', () => {
    const snapLines = resizeSnapLines(panel, viewport)
    // top-left grows leftward (offset.x) and shrinks from the top (offset.y),
    // with deltas chosen to stay clear of every rail.
    const result = resizePanelRect({
      startRect,
      handle: 'top-left',
      dx: -40,
      dy: 30,
      constraints,
      viewport,
      snapLines,
      locks: { x: null, y: null },
    })
    expect(result).toEqual({
      size: { width: 640, height: 370 },
      offset: { x: -20, y: -8 },
      activeLines: { x: [], y: [] },
      locks: { x: null, y: null },
    })
  })

  it('keeps a held rail lock until the pointer breaks out of the resist zone', () => {
    const snapLines = resizeSnapLines(panel, viewport)
    // Incoming lock on the right rail (900) holds the edge while inside 12px...
    const held = resizePanelRect({
      startRect,
      handle: 'right',
      dx: 8,
      dy: 0,
      constraints,
      viewport,
      snapLines,
      locks: { x: 900, y: null },
    })
    expect(held.size.width).toBe(600)
    expect(held.activeLines.x).toEqual([900])
    expect(held.locks.x).toBe(900)

    // ...and releases once the pointer travels past the resist distance.
    const released = resizePanelRect({
      startRect,
      handle: 'right',
      dx: 40,
      dy: 0,
      constraints,
      viewport,
      snapLines,
      locks: { x: 900, y: null },
    })
    expect(released.size.width).toBe(640)
    expect(released.activeLines.x).toEqual([])
    expect(released.locks.x).toBeNull()
  })
})
