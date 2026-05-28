import { describe, expect, it } from 'bun:test'

import {
  clampPanelOffset,
  guideColumns,
  guideRows,
  panelRailOffsets,
  resistPanelOffset,
} from '../src/floatingPanelGeometry'

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
})
