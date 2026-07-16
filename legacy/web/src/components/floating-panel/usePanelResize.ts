import {
  useRef,
  useState,
  type Dispatch,
  type RefObject,
  type SetStateAction,
  type PointerEvent as ReactPointerEvent,
} from 'react'

import {
  type ActiveResizeLines,
  type PanelOffset,
  type ResizeHandle,
  type ResizeSnapLines,
  clampPanelOffset,
  expandPanelToGrid,
  resizeGridLines,
  resizePanelRect,
} from '@/floatingPanelGeometry'
import {
  type FloatingPanelSizePreset,
  floatingPanelResizeConstraints,
} from '@/floatingPanelLayout'

import { RESIZE_DOUBLE_CLICK_MS, RESIZE_DOUBLE_CLICK_SLOP } from './constants'
import { persistPanelOffset, persistPanelSize, viewportSize } from './geometry'
import type { PanelSize } from './types'

export function usePanelResize({
  isExpanded,
  panelRef,
  setPanelOffset,
  setPanelSize,
  sizePreset,
  sizeStorageKey,
  storageKey,
}: {
  isExpanded: boolean
  panelRef: RefObject<HTMLDivElement | null>
  setPanelOffset: Dispatch<SetStateAction<PanelOffset>>
  setPanelSize: Dispatch<SetStateAction<PanelSize | null>>
  sizePreset: FloatingPanelSizePreset | undefined
  sizeStorageKey: string
  storageKey: string
}) {
  const [isResizing, setIsResizing] = useState(false)
  const [resizeHandle, setResizeHandle] = useState<ResizeHandle | null>(null)
  const [resizeLines, setResizeLines] = useState<ResizeSnapLines | null>(null)
  const [activeResizeLines, setActiveResizeLines] = useState<ActiveResizeLines>(
    { x: [], y: [] },
  )
  const lastResizeClickRef = useRef<{
    handle: ResizeHandle
    time: number
    x: number
    y: number
  } | null>(null)
  const resizeRef = useRef<{
    pointerId: number
    handle: ResizeHandle
    startRect: { left: number; top: number; width: number; height: number }
    startX: number
    startY: number
    snapLines: ResizeSnapLines
    lockX: number | null
    lockY: number | null
  } | null>(null)

  // Grow the panel out to the nearest grid lines for the double-clicked handle.
  // Holding Option/Alt expands the opposite edge of each affected axis too.
  function expandResizeToGrid(handle: ResizeHandle, both: boolean) {
    const rect = panelRef.current?.getBoundingClientRect()
    if (!rect) {
      return
    }
    const viewport = viewportSize()
    const { size, offset } = expandPanelToGrid({
      rect: {
        left: rect.left,
        top: rect.top,
        width: rect.width,
        height: rect.height,
      },
      handle,
      both,
      grid: resizeGridLines(viewport),
      viewport,
    })
    const clampedOffset = clampPanelOffset(offset, size, viewport)
    setPanelSize(size)
    setPanelOffset(clampedOffset)
    persistPanelSize(sizeStorageKey, size)
    persistPanelOffset(storageKey, clampedOffset)
  }

  function handleResizeStart(
    handle: ResizeHandle,
    event: ReactPointerEvent<HTMLDivElement>,
  ) {
    if (isExpanded) {
      return
    }
    event.preventDefault()
    event.stopPropagation()

    // Detect a double-click on the same handle manually: preventDefault() above
    // suppresses the native dblclick event, and we want the gesture scoped to
    // this handle anyway. A double-click grows to the grid instead of dragging.
    const last = lastResizeClickRef.current
    const isDoubleClick =
      last !== null &&
      last.handle === handle &&
      event.timeStamp - last.time < RESIZE_DOUBLE_CLICK_MS &&
      Math.abs(event.clientX - last.x) < RESIZE_DOUBLE_CLICK_SLOP &&
      Math.abs(event.clientY - last.y) < RESIZE_DOUBLE_CLICK_SLOP
    if (isDoubleClick) {
      lastResizeClickRef.current = null
      // Defensively end any gesture the first click may have left in flight, so
      // the expand path is self-contained regardless of pointerup ordering.
      const inFlight = resizeRef.current
      if (inFlight) {
        if (event.currentTarget.hasPointerCapture(inFlight.pointerId)) {
          event.currentTarget.releasePointerCapture(inFlight.pointerId)
        }
        resizeRef.current = null
        setIsResizing(false)
        setResizeHandle(null)
        setResizeLines(null)
        setActiveResizeLines({ x: [], y: [] })
      }
      expandResizeToGrid(handle, event.altKey)
      return
    }
    lastResizeClickRef.current = {
      handle,
      time: event.timeStamp,
      x: event.clientX,
      y: event.clientY,
    }

    const rect = panelRef.current?.getBoundingClientRect()
    if (!rect) {
      return
    }
    event.currentTarget.setPointerCapture(event.pointerId)
    const startRect = {
      left: rect.left,
      top: rect.top,
      width: rect.width,
      height: rect.height,
    }
    // Snap targets are the static, viewport-anchored grid — the same for every
    // window, so panels can be standardized to identical dimensions.
    const snapLines = resizeGridLines(viewportSize())
    resizeRef.current = {
      pointerId: event.pointerId,
      handle,
      startRect,
      startX: event.clientX,
      startY: event.clientY,
      snapLines,
      lockX: null,
      lockY: null,
    }
    setIsResizing(true)
    setResizeHandle(handle)
    setResizeLines(snapLines)
    setActiveResizeLines({ x: [], y: [] })
    // Pin the rendered size to the measured pixels so resizing continues from
    // exactly where the preset left off, with no visual jump on the first move.
    setPanelSize({ width: rect.width, height: rect.height })
  }

  function handleResizeMove(event: ReactPointerEvent<HTMLDivElement>) {
    const resize = resizeRef.current
    if (!resize || resize.pointerId !== event.pointerId) {
      return
    }
    const viewport = viewportSize()
    const result = resizePanelRect({
      startRect: resize.startRect,
      handle: resize.handle,
      dx: event.clientX - resize.startX,
      dy: event.clientY - resize.startY,
      constraints: floatingPanelResizeConstraints(sizePreset, viewport),
      viewport,
      snapLines: resize.snapLines,
      locks: { x: resize.lockX, y: resize.lockY },
    })
    resize.lockX = result.locks.x
    resize.lockY = result.locks.y
    setPanelSize(result.size)
    setPanelOffset(result.offset)
    setActiveResizeLines(result.activeLines)
  }

  function handleResizeEnd(event: ReactPointerEvent<HTMLDivElement>) {
    const resize = resizeRef.current
    if (!resize || resize.pointerId !== event.pointerId) {
      return
    }
    resizeRef.current = null
    setIsResizing(false)
    setResizeHandle(null)
    setResizeLines(null)
    setActiveResizeLines({ x: [], y: [] })
    setPanelSize((size) => {
      if (size) {
        persistPanelSize(sizeStorageKey, size)
      }
      return size
    })
    setPanelOffset((offset) => {
      persistPanelOffset(storageKey, offset)
      return offset
    })
  }

  return {
    activeResizeLines,
    handleResizeEnd,
    handleResizeMove,
    handleResizeStart,
    isResizing,
    resizeHandle,
    resizeLines,
  }
}
