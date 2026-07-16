import type { PanelGeometry, PanelOffset } from '@/floatingPanelGeometry'
import { isFiniteOffset } from '@/floatingPanelGeometry'
import type { ViewportSize } from '@/floatingPanelLayout'

import type { PanelSize } from './types'

export function readStoredPanelOffset(storageKey: string): PanelOffset {
  if (typeof window === 'undefined') {
    return { x: 0, y: 0 }
  }
  try {
    const parsed = JSON.parse(window.localStorage.getItem(storageKey) ?? 'null')
    return isFiniteOffset(parsed) ? parsed : { x: 0, y: 0 }
  } catch {
    return { x: 0, y: 0 }
  }
}

export function persistPanelOffset(storageKey: string, offset: PanelOffset) {
  if (typeof window === 'undefined') {
    return
  }
  try {
    window.localStorage.setItem(storageKey, JSON.stringify(offset))
  } catch {
    // Placement is a preference; failing to persist should not break the panel.
  }
}

export function readStoredPanelSize(storageKey: string): PanelSize | null {
  if (typeof window === 'undefined') {
    return null
  }
  try {
    const parsed = JSON.parse(window.localStorage.getItem(storageKey) ?? 'null')
    if (
      parsed &&
      typeof parsed === 'object' &&
      typeof parsed.width === 'number' &&
      typeof parsed.height === 'number' &&
      Number.isFinite(parsed.width) &&
      Number.isFinite(parsed.height)
    ) {
      return { width: parsed.width, height: parsed.height }
    }
    return null
  } catch {
    return null
  }
}

export function persistPanelSize(storageKey: string, size: PanelSize) {
  if (typeof window === 'undefined') {
    return
  }
  try {
    window.localStorage.setItem(storageKey, JSON.stringify(size))
  } catch {
    // Size is a preference; failing to persist should not break the panel.
  }
}

export function viewportSize(): ViewportSize {
  return { width: window.innerWidth, height: window.innerHeight }
}

export function clampNumber(value: number, min: number, max: number): number {
  return Math.min(Math.max(value, Math.min(min, max)), Math.max(min, max))
}

export function panelGeometry(panel: DOMRect): PanelGeometry {
  return { width: panel.width, height: panel.height }
}
