import {
  ExternalLink,
  GripHorizontal,
  Maximize2,
  Minimize2,
  Pin,
  X,
} from 'lucide-react'
import type { CSSProperties } from 'react'
import { useEffect, useRef, useState } from 'react'

import {
  type ActiveRails,
  type ActiveResizeLines,
  type GuideLayout,
  type PanelGeometry,
  type PanelOffset,
  type ResizeHandle,
  type ResizeSnapLines,
  clampPanelOffset,
  expandPanelToGrid,
  guideColumns,
  guideRows,
  isFiniteOffset,
  resistPanelOffset,
  resizeGridCell,
  resizeGridLines,
  resizePanelRect,
} from '@/floatingPanelGeometry'
import {
  type FloatingPanelSizePreset,
  floatingPanelResizeConstraints,
  floatingPanelSizeStyle,
  type ViewportSize,
} from '@/floatingPanelLayout'
import { cn } from '@/lib/utils'

type PanelSize = { width: number; height: number }

// Edge and corner resize handles. Corners drive both axes; corners sit above the
// edge handles so the overlap at the borders resolves to a 2-D drag.
const RESIZE_HANDLES: {
  handle: ResizeHandle
  className: string
  corner: boolean
}[] = [
  {
    handle: 'top',
    className: 'inset-x-0 top-0 h-1.5 cursor-ns-resize',
    corner: false,
  },
  {
    handle: 'bottom',
    className: 'inset-x-0 bottom-0 h-1.5 cursor-ns-resize',
    corner: false,
  },
  {
    handle: 'left',
    className: 'inset-y-0 left-0 w-1.5 cursor-ew-resize',
    corner: false,
  },
  {
    handle: 'right',
    className: 'inset-y-0 right-0 w-1.5 cursor-ew-resize',
    corner: false,
  },
  {
    handle: 'top-left',
    className: 'left-0 top-0 size-3 cursor-nwse-resize',
    corner: true,
  },
  {
    handle: 'top-right',
    className: 'right-0 top-0 size-3 cursor-nesw-resize',
    corner: true,
  },
  {
    handle: 'bottom-left',
    className: 'bottom-0 left-0 size-3 cursor-nesw-resize',
    corner: true,
  },
  {
    handle: 'bottom-right',
    className: 'bottom-0 right-0 size-3 cursor-nwse-resize',
    corner: true,
  },
]

const GUIDE_LINE_ACTIVE_CLASS =
  'bg-[color-mix(in_oklab,var(--brand-coral)_46%,transparent)]'
const GUIDE_LINE_IDLE_CLASS =
  'bg-[color-mix(in_oklab,var(--foreground)_14%,transparent)]'
// The resize grid shows many more lines than the movement rails, so its idle
// lines are fainter to stay calm.
const GRID_LINE_IDLE_CLASS =
  'bg-[color-mix(in_oklab,var(--foreground)_8%,transparent)]'

// Don't start a header drag from interactive controls or the grip (the grip has
// its own drag handlers); empty header space drags the panel.
const HEADER_NO_DRAG_SELECTOR =
  'button, a, input, textarea, select, label, [contenteditable], [data-no-drag]'

// Manual double-click detection on a resize handle (preventDefault suppresses
// the native dblclick): a second press on the same handle within this window
// and pointer slop grows the panel to the grid instead of dragging.
const RESIZE_DOUBLE_CLICK_MS = 350
const RESIZE_DOUBLE_CLICK_SLOP = 6

interface FloatingPanelProps {
  children: React.ReactNode
  className?: string
  closeIgnoreSelector?: string
  header: React.ReactNode
  headerClassName?: string
  panelLabel: string
  sizePreset?: FloatingPanelSizePreset
  storageKey: string
  zIndexClassName?: string
  onClose: () => void
  onOpenInWindow?: () => void
}

function readStoredPanelOffset(storageKey: string): PanelOffset {
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

function persistPanelOffset(storageKey: string, offset: PanelOffset) {
  if (typeof window === 'undefined') {
    return
  }
  try {
    window.localStorage.setItem(storageKey, JSON.stringify(offset))
  } catch {
    // Placement is a preference; failing to persist should not break the panel.
  }
}

function readStoredPanelSize(storageKey: string): PanelSize | null {
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

function persistPanelSize(storageKey: string, size: PanelSize) {
  if (typeof window === 'undefined') {
    return
  }
  try {
    window.localStorage.setItem(storageKey, JSON.stringify(size))
  } catch {
    // Size is a preference; failing to persist should not break the panel.
  }
}

function viewportSize(): ViewportSize {
  return { width: window.innerWidth, height: window.innerHeight }
}

function clampNumber(value: number, min: number, max: number): number {
  return Math.min(Math.max(value, Math.min(min, max)), Math.max(min, max))
}

function panelGeometry(panel: DOMRect): PanelGeometry {
  return { width: panel.width, height: panel.height }
}

export function FloatingPanel({
  children,
  className,
  closeIgnoreSelector,
  header,
  headerClassName,
  panelLabel,
  sizePreset,
  storageKey,
  zIndexClassName = 'z-[70]',
  onClose,
  onOpenInWindow,
}: FloatingPanelProps) {
  const sizeStorageKey = `${storageKey}:size`
  const [isPinned, setIsPinned] = useState(false)
  const [isExpanded, setIsExpanded] = useState(false)
  const [isDragging, setIsDragging] = useState(false)
  const [activeRails, setActiveRails] = useState<ActiveRails>({
    x: null,
    y: null,
  })
  const [guideLayout, setGuideLayout] = useState<GuideLayout | null>(null)
  const [panelOffset, setPanelOffset] = useState(() =>
    readStoredPanelOffset(storageKey),
  )
  const [panelSize, setPanelSize] = useState<PanelSize | null>(() =>
    readStoredPanelSize(`${storageKey}:size`),
  )
  const [isResizing, setIsResizing] = useState(false)
  const [resizeHandle, setResizeHandle] = useState<ResizeHandle | null>(null)
  const [resizeLines, setResizeLines] = useState<ResizeSnapLines | null>(null)
  const [activeResizeLines, setActiveResizeLines] = useState<ActiveResizeLines>(
    { x: [], y: [] },
  )
  const panelRef = useRef<HTMLDivElement>(null)
  const lastResizeClickRef = useRef<{
    handle: ResizeHandle
    time: number
    x: number
    y: number
  } | null>(null)
  const dragRef = useRef<{
    lockedX?: number | null
    lockedY?: number | null
    pointerId: number
    startX: number
    startY: number
    originX: number
    originY: number
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

  useEffect(() => {
    function handleKeyDown(event: KeyboardEvent) {
      if (event.defaultPrevented) {
        return
      }
      if (event.key === 'Escape') {
        event.preventDefault()
        onClose()
      }
    }

    window.addEventListener('keydown', handleKeyDown)
    return () => window.removeEventListener('keydown', handleKeyDown)
  }, [onClose])

  useEffect(() => {
    function handlePointerDown(event: PointerEvent) {
      if (isPinned) {
        return
      }
      const target = event.target
      if (!(target instanceof Node)) {
        return
      }
      if (panelRef.current?.contains(target)) {
        return
      }
      if (
        closeIgnoreSelector &&
        target instanceof Element &&
        target.closest(closeIgnoreSelector)
      ) {
        return
      }
      onClose()
    }

    window.addEventListener('pointerdown', handlePointerDown, true)
    return () =>
      window.removeEventListener('pointerdown', handlePointerDown, true)
  }, [closeIgnoreSelector, isPinned, onClose])

  function handleHeaderPointerDown(event: React.PointerEvent<HTMLDivElement>) {
    if (
      event.target instanceof Element &&
      event.target.closest(HEADER_NO_DRAG_SELECTOR)
    ) {
      return
    }
    handleDragStart(event)
  }

  function handleDragStart(event: React.PointerEvent<HTMLElement>) {
    if (isExpanded) {
      return
    }
    event.preventDefault()
    event.currentTarget.setPointerCapture(event.pointerId)
    const panel = panelRef.current?.getBoundingClientRect()
    setIsDragging(true)
    setGuideLayout(
      panel
        ? {
            columns: guideColumns(panelGeometry(panel), viewportSize()),
            rows: guideRows(panelGeometry(panel), viewportSize()),
          }
        : null,
    )
    dragRef.current = {
      lockedX: null,
      lockedY: null,
      pointerId: event.pointerId,
      startX: event.clientX,
      startY: event.clientY,
      originX: panelOffset.x,
      originY: panelOffset.y,
    }
  }

  function handleDragMove(event: React.PointerEvent<HTMLElement>) {
    const drag = dragRef.current
    if (!drag || drag.pointerId !== event.pointerId) {
      return
    }
    const panel = panelRef.current?.getBoundingClientRect()
    const nextOffset = {
      x: drag.originX + event.clientX - drag.startX,
      y: drag.originY + event.clientY - drag.startY,
    }
    if (!panel) {
      setPanelOffset(nextOffset)
      setActiveRails({ x: null, y: null })
      return
    }
    const resisted = resistPanelOffset(
      nextOffset,
      panelGeometry(panel),
      viewportSize(),
      drag,
    )
    setPanelOffset(resisted.offset)
    setActiveRails(resisted.activeRails)
  }

  function handleDragEnd(event: React.PointerEvent<HTMLElement>) {
    const drag = dragRef.current
    if (drag?.pointerId === event.pointerId) {
      const panel = panelRef.current?.getBoundingClientRect()
      const rawOffset = {
        x: drag.originX + event.clientX - drag.startX,
        y: drag.originY + event.clientY - drag.startY,
      }
      const resisted = panel
        ? resistPanelOffset(
            rawOffset,
            panelGeometry(panel),
            viewportSize(),
            drag,
          )
        : { activeRails: { x: null, y: null }, offset: rawOffset }
      const nextOffset = resisted.offset
      dragRef.current = null
      setIsDragging(false)
      setGuideLayout(null)
      setActiveRails({ x: null, y: null })
      setPanelOffset(nextOffset)
      persistPanelOffset(storageKey, nextOffset)
    }
  }

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
    event: React.PointerEvent<HTMLDivElement>,
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

  function handleResizeMove(event: React.PointerEvent<HTMLDivElement>) {
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

  function handleResizeEnd(event: React.PointerEvent<HTMLDivElement>) {
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

  const floatingSizeStyle: CSSProperties =
    !isExpanded && sizePreset ? floatingPanelSizeStyle(sizePreset) : {}
  const resizeSizeStyle: CSSProperties =
    !isExpanded && panelSize
      ? { width: `${panelSize.width}px`, height: `${panelSize.height}px` }
      : {}

  // Only draw the grid axis being resized: vertical lines while changing width,
  // horizontal while changing height, both at a corner.
  const resizeShowsX =
    resizeHandle !== null &&
    (resizeHandle.includes('left') || resizeHandle.includes('right'))
  const resizeShowsY =
    resizeHandle !== null &&
    (resizeHandle.includes('top') || resizeHandle.includes('bottom'))
  const resizeCells =
    isResizing && panelSize
      ? (() => {
          const cell = resizeGridCell(viewportSize())
          return {
            columns:
              cell.width > 0 ? Math.round(panelSize.width / cell.width) : 0,
            rows:
              cell.height > 0 ? Math.round(panelSize.height / cell.height) : 0,
          }
        })()
      : null

  return (
    <div
      className={cn(
        'pointer-events-none fixed inset-0 flex items-start justify-center px-4',
        isExpanded ? 'pt-4' : 'pt-[54px]',
        zIndexClassName,
      )}
      aria-live="polite"
    >
      {isDragging && guideLayout && (
        <div className="pointer-events-none fixed inset-0">
          {guideLayout.columns.map((column) => {
            const lineClass =
              activeRails.x === column.rail
                ? GUIDE_LINE_ACTIVE_CLASS
                : GUIDE_LINE_IDLE_CLASS
            return (
              <div
                key={`column:${column.rail}`}
                className="absolute top-0 h-full"
                style={{ left: column.left, width: column.width }}
              >
                <div
                  className={`absolute left-0 top-0 h-full w-px ${lineClass}`}
                />
                <div
                  className={`absolute right-0 top-0 h-full w-px ${lineClass}`}
                />
              </div>
            )
          })}
          {guideLayout.rows.map((row) => {
            const lineClass =
              activeRails.y === row.rail
                ? GUIDE_LINE_ACTIVE_CLASS
                : GUIDE_LINE_IDLE_CLASS
            return (
              <div
                key={`row:${row.rail}`}
                className="absolute left-0 w-full"
                style={{ height: row.height, top: row.top }}
              >
                <div
                  className={`absolute left-0 top-0 h-px w-full ${lineClass}`}
                />
                <div
                  className={`absolute bottom-0 left-0 h-px w-full ${lineClass}`}
                />
              </div>
            )
          })}
        </div>
      )}
      {isResizing && resizeLines && (
        <div className="pointer-events-none fixed inset-0">
          {resizeShowsX &&
            resizeLines.x.map((x) => (
              <div
                key={`resize-x:${x}`}
                className={`absolute top-0 h-full w-px ${
                  activeResizeLines.x.includes(x)
                    ? GUIDE_LINE_ACTIVE_CLASS
                    : GRID_LINE_IDLE_CLASS
                }`}
                style={{ left: x }}
              />
            ))}
          {resizeShowsY &&
            resizeLines.y.map((y) => (
              <div
                key={`resize-y:${y}`}
                className={`absolute left-0 h-px w-full ${
                  activeResizeLines.y.includes(y)
                    ? GUIDE_LINE_ACTIVE_CLASS
                    : GRID_LINE_IDLE_CLASS
                }`}
                style={{ top: y }}
              />
            ))}
        </div>
      )}
      <div
        ref={panelRef}
        className={cn(
          'pointer-events-auto relative w-full overflow-hidden rounded-[14px] border [border-color:color-mix(in_oklab,var(--brand-coral)_22%,var(--border))] bg-[linear-gradient(135deg,color-mix(in_oklab,var(--brand-coral)_14%,var(--panel))_0%,color-mix(in_oklab,var(--ring)_7%,var(--panel))_50%,var(--panel)_100%)] text-foreground shadow-[0_28px_80px_rgb(0_0_0/0.24)] backdrop-blur-[24px] backdrop-saturate-150 dark:shadow-[0_28px_80px_rgb(0_0_0/0.48)]',
          className,
          isExpanded
            ? 'h-[calc(100vh-2rem)] max-h-[calc(100vh-2rem)] max-w-[calc(100vw-2rem)]'
            : 'max-h-[calc(100vh-2rem)] max-w-[calc(100vw-2rem)] min-h-48 min-w-72',
          // Animate double-click grow / restore, but never during a live drag or
          // resize (that must track the pointer instantly).
          !isExpanded &&
            !isDragging &&
            !isResizing &&
            'transition-[transform,width,height] duration-150 ease-out',
        )}
        style={{
          ...floatingSizeStyle,
          ...resizeSizeStyle,
          transform: isExpanded
            ? undefined
            : `translate(${panelOffset.x}px, ${panelOffset.y}px)`,
        }}
      >
        <div
          className={cn(
            'border-b px-3 [border-color:color-mix(in_oklab,var(--brand-coral)_12%,var(--border))]',
            headerClassName,
          )}
        >
          <div
            className={cn(
              'flex items-center',
              !isExpanded && 'cursor-grab touch-none active:cursor-grabbing',
            )}
            onPointerDown={isExpanded ? undefined : handleHeaderPointerDown}
            onPointerMove={isExpanded ? undefined : handleDragMove}
            onPointerUp={isExpanded ? undefined : handleDragEnd}
            onPointerCancel={isExpanded ? undefined : handleDragEnd}
          >
            <div className="flex shrink-0 items-center gap-0.5">
              <button
                type="button"
                title={`Move ${panelLabel}`}
                className="ph-focus-ring flex size-7 cursor-grab touch-none items-center justify-center rounded-[6px] text-muted-foreground transition-colors hover:bg-[color-mix(in_oklab,var(--brand-coral)_11%,transparent)] hover:text-foreground active:cursor-grabbing"
                onPointerDown={handleDragStart}
                onPointerMove={handleDragMove}
                onPointerUp={handleDragEnd}
                onPointerCancel={handleDragEnd}
              >
                <GripHorizontal size={15} strokeWidth={1.8} />
              </button>
              <button
                type="button"
                aria-pressed={isPinned}
                title={isPinned ? `Unpin ${panelLabel}` : `Pin ${panelLabel}`}
                className={cn(
                  'ph-focus-ring flex size-7 items-center justify-center rounded-[6px] text-muted-foreground transition-colors hover:bg-[color-mix(in_oklab,var(--brand-coral)_11%,transparent)] hover:text-foreground',
                  isPinned &&
                    'bg-[color-mix(in_oklab,var(--brand-coral)_15%,transparent)] text-foreground',
                )}
                onClick={() => setIsPinned((pinned) => !pinned)}
              >
                <Pin size={15} strokeWidth={1.8} />
              </button>
              {onOpenInWindow && (
                <button
                  type="button"
                  title={`Open ${panelLabel} in window`}
                  className="ph-focus-ring flex size-7 items-center justify-center rounded-[6px] text-muted-foreground transition-colors hover:bg-[color-mix(in_oklab,var(--brand-coral)_11%,transparent)] hover:text-foreground"
                  onClick={onOpenInWindow}
                >
                  <ExternalLink size={15} strokeWidth={1.8} />
                </button>
              )}
              <button
                type="button"
                aria-pressed={isExpanded}
                title={
                  isExpanded ? `Restore ${panelLabel}` : `Expand ${panelLabel}`
                }
                className={cn(
                  'ph-focus-ring flex size-7 items-center justify-center rounded-[6px] text-muted-foreground transition-colors hover:bg-[color-mix(in_oklab,var(--brand-coral)_11%,transparent)] hover:text-foreground',
                  isExpanded &&
                    'bg-[color-mix(in_oklab,var(--brand-coral)_15%,transparent)] text-foreground',
                )}
                onClick={() => setIsExpanded((expanded) => !expanded)}
              >
                {isExpanded ? (
                  <Minimize2 size={15} strokeWidth={1.8} />
                ) : (
                  <Maximize2 size={15} strokeWidth={1.8} />
                )}
              </button>
            </div>
            <div className="min-w-0 flex-1">{header}</div>
            <button
              type="button"
              aria-label={`Close ${panelLabel}`}
              className="ph-focus-ring flex size-7 shrink-0 items-center justify-center rounded-[6px] text-muted-foreground transition-colors hover:bg-[color-mix(in_oklab,var(--brand-coral)_11%,transparent)] hover:text-foreground"
              onClick={onClose}
            >
              <X size={15} strokeWidth={1.8} />
            </button>
          </div>
        </div>
        {children}
        {/*
          Edge/corner resize handles sit above the content (z-20/z-30) so they
          stay reachable over panels whose body captures the pointer (e.g. the
          command list). The top edge and top corners deliberately overlay the
          header's top few pixels, so a drag started in that thin strip resizes
          rather than moves — the expected window-chrome tradeoff.
        */}
        {!isExpanded &&
          RESIZE_HANDLES.map(({ handle, className, corner }) => (
            <div
              key={handle}
              role="presentation"
              aria-hidden="true"
              className={cn(
                'absolute touch-none transition-colors duration-150',
                corner ? 'z-30' : 'z-20',
                // Faint brand-coral hint on hover (and while dragging, since the
                // pointer stays captured on the handle); corners read a touch
                // stronger to advertise the 2-D drag.
                corner
                  ? 'hover:bg-[color-mix(in_oklab,var(--brand-coral)_38%,transparent)]'
                  : 'hover:bg-[color-mix(in_oklab,var(--brand-coral)_26%,transparent)]',
                className,
              )}
              onPointerDown={(event) => handleResizeStart(handle, event)}
              onPointerMove={handleResizeMove}
              onPointerUp={handleResizeEnd}
              onPointerCancel={handleResizeEnd}
            />
          ))}
        {isResizing && resizeCells && (
          <div className="pointer-events-none absolute left-1/2 top-1/2 z-40 -translate-x-1/2 -translate-y-1/2 rounded-md border [border-color:color-mix(in_oklab,var(--brand-coral)_30%,var(--border))] bg-[color-mix(in_oklab,var(--panel)_85%,transparent)] px-2 py-1 font-mono text-[11px] font-medium text-foreground/80 shadow-sm backdrop-blur-sm">
            {resizeCells.columns} × {resizeCells.rows}
          </div>
        )}
      </div>
    </div>
  )
}
