import { describe, expect, it } from 'bun:test'

import {
  floatingPanelSizeStyle,
  resolveFloatingPanelSize,
} from '../src/floatingPanelLayout'

describe('floating panel size grid', () => {
  it('resolves compose defaults from the shared viewport grid', () => {
    const size = resolveFloatingPanelSize('compose', {
      width: 1200,
      height: 800,
    })

    expect(Math.round(size.width)).toBe(779)
    expect(Math.round(size.height ?? 0)).toBe(548)
  })

  it('keeps command palette compact and height-content driven', () => {
    const size = resolveFloatingPanelSize('command', {
      width: 1200,
      height: 800,
    })

    expect(size).toEqual({ width: 584 })
  })

  it('clamps defaults to the usable viewport on small screens', () => {
    const size = resolveFloatingPanelSize('compose', {
      width: 500,
      height: 500,
    })

    expect(size).toEqual({ width: 468, height: 430 })
  })

  it('emits CSS constraints that still allow native arbitrary resize', () => {
    const style = floatingPanelSizeStyle('compose')

    expect(style).toMatchObject({
      width:
        'clamp(min(560px, calc(100vw - 32px)), calc(calc(100vw - 32px) * 0.6667), min(780px, calc(100vw - 32px)))',
      height:
        'clamp(min(460px, calc(100vh - 70px)), calc(calc(100vh - 70px) * 0.75), min(640px, calc(100vh - 70px)))',
      maxWidth: 'calc(100vw - 32px)',
      maxHeight: 'calc(100vh - 70px)',
      minWidth: 'min(560px, calc(100vw - 32px))',
      minHeight: 'min(460px, calc(100vh - 70px))',
    })
    expect('resize' in style).toBe(false)
  })
})
