import { GripHorizontal, Pin, X } from 'lucide-react'
import { useEffect, useRef, useState } from 'react'

import { cn } from '@/lib/utils'

const PANEL_TOP_OFFSET = 54
const PANEL_SCREEN_MARGIN = 16
const RAIL_RESISTANCE_DISTANCE = 12

interface PanelOffset {
  x: number
  y: number
}

interface ActiveRails {
  x: number | null
  y: number | null
}

interface PanelRails {
  x: number[]
  y: number[]
}

interface GuideColumn {
  left: number
  rail: number
  width: number
}

interface GuideRow {
  height: number
  rail: number
  top: number
}

interface GuideLayout {
  columns: GuideColumn[]
  rows: GuideRow[]
}

interface FloatingPanelProps {
  children: React.ReactNode
  className?: string
  closeIgnoreSelector?: string
  header: React.ReactNode
  headerClassName?: string
  panelLabel: string
  storageKey: string
  zIndexClassName?: string
  onClose: () => void
}

function isFiniteOffset(value: unknown): value is PanelOffset {
  return (
    typeof value === 'object' &&
    value !== null &&
    'x' in value &&
    'y' in value &&
    typeof value.x === 'number' &&
    typeof value.y === 'number' &&
    Number.isFinite(value.x) &&
    Number.isFinite(value.y)
  )
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

function clamp(value: number, min: number, max: number): number {
  return Math.min(Math.max(value, min), max)
}

function clampPanelOffset(offset: PanelOffset, panel: DOMRect): PanelOffset {
  const viewportWidth = window.innerWidth
  const viewportHeight = window.innerHeight
  const baseLeft = (viewportWidth - panel.width) / 2
  const minX = PANEL_SCREEN_MARGIN - baseLeft
  const maxX = viewportWidth - PANEL_SCREEN_MARGIN - panel.width - baseLeft
  const minY = PANEL_SCREEN_MARGIN - PANEL_TOP_OFFSET
  const maxY =
    viewportHeight - PANEL_SCREEN_MARGIN - panel.height - PANEL_TOP_OFFSET

  return {
    x: clamp(offset.x, Math.min(minX, maxX), Math.max(minX, maxX)),
    y: clamp(offset.y, Math.min(minY, maxY), Math.max(minY, maxY)),
  }
}

function uniqueRails(values: number[]): number[] {
  const rails: number[] = []
  for (const value of values) {
    if (!rails.some((rail) => Math.abs(rail - value) < 1)) {
      rails.push(value)
    }
  }
  return rails
}

function panelRailOffsets(panel: DOMRect): PanelRails {
  const viewportWidth = window.innerWidth
  const viewportHeight = window.innerHeight
  const baseCenterX = viewportWidth / 2
  const baseCenterY = PANEL_TOP_OFFSET + panel.height / 2
  const horizontalCenters = [
    viewportWidth / 6,
    viewportWidth / 2,
    (viewportWidth * 5) / 6,
  ].map((center) =>
    clamp(
      center,
      PANEL_SCREEN_MARGIN + panel.width / 2,
      viewportWidth - PANEL_SCREEN_MARGIN - panel.width / 2,
    ),
  )
  const verticalCenters = [viewportHeight / 4, (viewportHeight * 3) / 4].map(
    (center) =>
      clamp(
        center,
        PANEL_SCREEN_MARGIN + panel.height / 2,
        viewportHeight - PANEL_SCREEN_MARGIN - panel.height / 2,
      ),
  )

  return {
    x: uniqueRails(horizontalCenters.map((centerX) => centerX - baseCenterX)),
    y: uniqueRails(verticalCenters.map((centerY) => centerY - baseCenterY)),
  }
}

function nearestRail(value: number, rails: number[]): number | null {
  let nearest: number | null = null
  let nearestDistance = Number.POSITIVE_INFINITY

  for (const rail of rails) {
    const distance = Math.abs(value - rail)
    if (distance < nearestDistance) {
      nearest = rail
      nearestDistance = distance
    }
  }

  return nearestDistance <= RAIL_RESISTANCE_DISTANCE ? nearest : null
}

function resistRail(
  value: number,
  locked: number | null | undefined,
  rails: number[],
): { active: number | null; locked: number | null; value: number } {
  if (locked !== null && locked !== undefined) {
    if (Math.abs(value - locked) <= RAIL_RESISTANCE_DISTANCE) {
      return { active: locked, locked, value: locked }
    }
  }

  const nearest = nearestRail(value, rails)
  if (nearest !== null) {
    return { active: nearest, locked: nearest, value: nearest }
  }

  return { active: null, locked: null, value }
}

function resistPanelOffset(
  offset: PanelOffset,
  panel: DOMRect,
  drag: { lockedX?: number | null; lockedY?: number | null },
): { activeRails: ActiveRails; offset: PanelOffset } {
  const clamped = clampPanelOffset(offset, panel)
  const rails = panelRailOffsets(panel)
  const x = resistRail(clamped.x, drag.lockedX, rails.x)
  const y = resistRail(clamped.y, drag.lockedY, rails.y)
  drag.lockedX = x.locked
  drag.lockedY = y.locked

  return {
    activeRails: { x: x.active, y: y.active },
    offset: { x: x.value, y: y.value },
  }
}

function guideColumns(panel: DOMRect): GuideColumn[] {
  const viewportWidth = window.innerWidth
  const baseCenterX = viewportWidth / 2

  return panelRailOffsets(panel).x.map((rail) => {
    const centerX = baseCenterX + rail
    return {
      left: centerX - panel.width / 2,
      rail,
      width: panel.width,
    }
  })
}

function guideRows(panel: DOMRect): GuideRow[] {
  const baseCenterY = PANEL_TOP_OFFSET + panel.height / 2

  return panelRailOffsets(panel).y.map((rail) => {
    const centerY = baseCenterY + rail
    return {
      height: panel.height,
      rail,
      top: centerY - panel.height / 2,
    }
  })
}

export function FloatingPanel({
  children,
  className,
  closeIgnoreSelector,
  header,
  headerClassName,
  panelLabel,
  storageKey,
  zIndexClassName = 'z-[70]',
  onClose,
}: FloatingPanelProps) {
  const [isPinned, setIsPinned] = useState(false)
  const [isDragging, setIsDragging] = useState(false)
  const [activeRails, setActiveRails] = useState<ActiveRails>({
    x: null,
    y: null,
  })
  const [guideLayout, setGuideLayout] = useState<GuideLayout | null>(null)
  const [panelOffset, setPanelOffset] = useState(() =>
    readStoredPanelOffset(storageKey),
  )
  const panelRef = useRef<HTMLDivElement>(null)
  const dragRef = useRef<{
    lockedX?: number | null
    lockedY?: number | null
    pointerId: number
    startX: number
    startY: number
    originX: number
    originY: number
  } | null>(null)

  useEffect(() => {
    function clampRestoredOffset() {
      const panel = panelRef.current?.getBoundingClientRect()
      if (!panel) {
        return
      }
      setPanelOffset((current) => {
        const clamped = clampPanelOffset(current, panel)
        if (clamped.x === current.x && clamped.y === current.y) {
          return current
        }
        persistPanelOffset(storageKey, clamped)
        return clamped
      })
    }

    clampRestoredOffset()
    window.addEventListener('resize', clampRestoredOffset)
    return () => window.removeEventListener('resize', clampRestoredOffset)
  }, [storageKey])

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

  function handleDragStart(event: React.PointerEvent<HTMLButtonElement>) {
    event.preventDefault()
    event.currentTarget.setPointerCapture(event.pointerId)
    const panel = panelRef.current?.getBoundingClientRect()
    setIsDragging(true)
    setGuideLayout(
      panel ? { columns: guideColumns(panel), rows: guideRows(panel) } : null,
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

  function handleDragMove(event: React.PointerEvent<HTMLButtonElement>) {
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
    const resisted = resistPanelOffset(nextOffset, panel, drag)
    setPanelOffset(resisted.offset)
    setActiveRails(resisted.activeRails)
  }

  function handleDragEnd(event: React.PointerEvent<HTMLButtonElement>) {
    const drag = dragRef.current
    if (drag?.pointerId === event.pointerId) {
      const panel = panelRef.current?.getBoundingClientRect()
      const rawOffset = {
        x: drag.originX + event.clientX - drag.startX,
        y: drag.originY + event.clientY - drag.startY,
      }
      const resisted = panel
        ? resistPanelOffset(rawOffset, panel, drag)
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

  return (
    <div
      className={cn(
        'pointer-events-none fixed inset-0 flex items-start justify-center px-4 pt-[54px]',
        zIndexClassName,
      )}
      aria-live="polite"
    >
      {isDragging && guideLayout && (
        <div className="pointer-events-none fixed inset-0">
          {guideLayout.columns.map((column) => {
            const active = activeRails.x === column.rail
            const lineClass = active
              ? 'bg-[color-mix(in_oklab,var(--brand-coral)_46%,transparent)]'
              : 'bg-[color-mix(in_oklab,var(--foreground)_14%,transparent)]'
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
            const active = activeRails.y === row.rail
            const lineClass = active
              ? 'bg-[color-mix(in_oklab,var(--brand-coral)_46%,transparent)]'
              : 'bg-[color-mix(in_oklab,var(--foreground)_14%,transparent)]'
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
      <div
        ref={panelRef}
        className={cn(
          'pointer-events-auto w-full overflow-hidden rounded-[14px] border [border-color:color-mix(in_oklab,var(--brand-coral)_22%,var(--border))] bg-[linear-gradient(135deg,color-mix(in_oklab,var(--brand-coral)_14%,var(--panel))_0%,color-mix(in_oklab,var(--ring)_7%,var(--panel))_50%,var(--panel)_100%)] text-foreground shadow-[0_28px_80px_rgb(0_0_0/0.24)] backdrop-blur-[24px] backdrop-saturate-150 dark:shadow-[0_28px_80px_rgb(0_0_0/0.48)]',
          className,
        )}
        style={{
          transform: `translate(${panelOffset.x}px, ${panelOffset.y}px)`,
        }}
      >
        <div
          className={cn(
            'border-b px-3 [border-color:color-mix(in_oklab,var(--brand-coral)_12%,var(--border))]',
            headerClassName,
          )}
        >
          <div className="flex items-center">
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
      </div>
    </div>
  )
}
