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
  type GuideLayout,
  type PanelGeometry,
  type PanelOffset,
  clampPanelOffset,
  guideColumns,
  guideRows,
  isFiniteOffset,
  resistPanelOffset,
} from '@/floatingPanelGeometry'
import {
  type FloatingPanelSizePreset,
  floatingPanelSizeStyle,
  type ViewportSize,
} from '@/floatingPanelLayout'
import { cn } from '@/lib/utils'

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

function viewportSize(): ViewportSize {
  return { width: window.innerWidth, height: window.innerHeight }
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
        const clamped = clampPanelOffset(
          current,
          panelGeometry(panel),
          viewportSize(),
        )
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
    const resisted = resistPanelOffset(
      nextOffset,
      panelGeometry(panel),
      viewportSize(),
      drag,
    )
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

  const floatingSizeStyle: CSSProperties =
    !isExpanded && sizePreset ? floatingPanelSizeStyle(sizePreset) : {}

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
          isExpanded
            ? 'h-[calc(100vh-2rem)] max-h-[calc(100vh-2rem)] max-w-[calc(100vw-2rem)]'
            : 'max-h-[calc(100vh-2rem)] max-w-[calc(100vw-2rem)] min-h-48 min-w-72 resize',
        )}
        style={{
          ...floatingSizeStyle,
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
      </div>
    </div>
  )
}
