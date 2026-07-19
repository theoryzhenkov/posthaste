import type { PanelGeometry, PanelOffset } from '@/components/floating/geometry'
import { isFiniteOffset } from '@/components/floating/geometry'
import type { ViewportSize } from '@/components/floating/geometry/layout'
import { readStorageItem, writeStorageItem } from '@/lib/ambient/storage'

import type { PanelSize } from './types'

// Persistence goes through the R8 storage seam: absent/blocked storage reads
// as null and swallows writes — geometry is a preference; failing to persist
// never breaks the panel.

function readStoredJson(storageKey: string): unknown {
  try {
    return JSON.parse(readStorageItem(storageKey) ?? 'null')
  } catch {
    return null
  }
}

export function readStoredPanelOffset(storageKey: string): PanelOffset {
  const parsed = readStoredJson(storageKey)
  return isFiniteOffset(parsed) ? parsed : { x: 0, y: 0 }
}

export function persistPanelOffset(storageKey: string, offset: PanelOffset) {
  writeStorageItem(storageKey, JSON.stringify(offset))
}

export function readStoredPanelSize(storageKey: string): PanelSize | null {
  const parsed = readStoredJson(storageKey)
  if (
    parsed &&
    typeof parsed === 'object' &&
    'width' in parsed &&
    'height' in parsed &&
    typeof parsed.width === 'number' &&
    typeof parsed.height === 'number' &&
    Number.isFinite(parsed.width) &&
    Number.isFinite(parsed.height)
  ) {
    return { width: parsed.width, height: parsed.height }
  }
  return null
}

export function persistPanelSize(storageKey: string, size: PanelSize) {
  writeStorageItem(storageKey, JSON.stringify(size))
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
