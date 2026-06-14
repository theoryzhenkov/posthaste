import { useEffect, useRef, useState } from 'react'

import { clampPanelOffset } from '@/floatingPanelGeometry'
import {
  type FloatingPanelSizePreset,
  floatingPanelResizeConstraints,
} from '@/floatingPanelLayout'

import {
  clampNumber,
  panelGeometry,
  persistPanelOffset,
  persistPanelSize,
  readStoredPanelOffset,
  readStoredPanelSize,
  viewportSize,
} from './geometry'
import type { PanelSize } from './types'

export function usePanelPlacement({
  sizePreset,
  storageKey,
}: {
  sizePreset: FloatingPanelSizePreset | undefined
  storageKey: string
}) {
  const sizeStorageKey = `${storageKey}:size`
  const panelRef = useRef<HTMLDivElement>(null)
  const [panelOffset, setPanelOffset] = useState(() =>
    readStoredPanelOffset(storageKey),
  )
  const [panelSize, setPanelSize] = useState<PanelSize | null>(() =>
    readStoredPanelSize(`${storageKey}:size`),
  )

  useEffect(() => {
    function clampRestoredPlacement() {
      const panel = panelRef.current?.getBoundingClientRect()
      if (!panel) {
        return
      }
      const viewport = viewportSize()
      // The size update below runs its updater before the offset updater in the
      // same render pass; capture the clamped size here so the offset is clamped
      // against the panel's new footprint rather than the stale measured rect.
      let effectiveGeometry = panelGeometry(panel)
      setPanelSize((current) => {
        if (!current) {
          return current
        }
        const constraints = floatingPanelResizeConstraints(sizePreset, viewport)
        const clamped = {
          width: clampNumber(
            current.width,
            constraints.minWidth,
            constraints.maxWidth,
          ),
          height: clampNumber(
            current.height,
            constraints.minHeight,
            constraints.maxHeight,
          ),
        }
        effectiveGeometry = clamped
        if (
          clamped.width === current.width &&
          clamped.height === current.height
        ) {
          return current
        }
        persistPanelSize(sizeStorageKey, clamped)
        return clamped
      })
      setPanelOffset((current) => {
        const clamped = clampPanelOffset(current, effectiveGeometry, viewport)
        if (clamped.x === current.x && clamped.y === current.y) {
          return current
        }
        persistPanelOffset(storageKey, clamped)
        return clamped
      })
    }

    clampRestoredPlacement()
    window.addEventListener('resize', clampRestoredPlacement)
    return () => window.removeEventListener('resize', clampRestoredPlacement)
  }, [storageKey, sizeStorageKey, sizePreset])

  return {
    panelOffset,
    panelRef,
    panelSize,
    setPanelOffset,
    setPanelSize,
    sizeStorageKey,
  }
}
