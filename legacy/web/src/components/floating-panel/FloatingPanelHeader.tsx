import {
  ExternalLink,
  GripHorizontal,
  Maximize2,
  Minimize2,
  Pin,
  X,
} from 'lucide-react'
import type {
  Dispatch,
  PointerEvent as ReactPointerEvent,
  ReactNode,
  SetStateAction,
} from 'react'

import { cn } from '@/lib/utils'

export function FloatingPanelHeader({
  header,
  headerClassName,
  isExpanded,
  isPinned,
  panelLabel,
  setIsExpanded,
  setIsPinned,
  onClose,
  onDragEnd,
  onDragMove,
  onDragStart,
  onHeaderPointerDown,
  onOpenInWindow,
}: {
  header: ReactNode
  headerClassName?: string
  isExpanded: boolean
  isPinned: boolean
  panelLabel: string
  setIsExpanded: Dispatch<SetStateAction<boolean>>
  setIsPinned: Dispatch<SetStateAction<boolean>>
  onClose: () => void
  onDragEnd: (event: ReactPointerEvent<HTMLElement>) => void
  onDragMove: (event: ReactPointerEvent<HTMLElement>) => void
  onDragStart: (event: ReactPointerEvent<HTMLElement>) => void
  onHeaderPointerDown: (event: ReactPointerEvent<HTMLDivElement>) => void
  onOpenInWindow?: () => void
}) {
  return (
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
        onPointerDown={isExpanded ? undefined : onHeaderPointerDown}
        onPointerMove={isExpanded ? undefined : onDragMove}
        onPointerUp={isExpanded ? undefined : onDragEnd}
        onPointerCancel={isExpanded ? undefined : onDragEnd}
      >
        <div className="flex shrink-0 items-center gap-0.5">
          <button
            type="button"
            title={`Move ${panelLabel}`}
            className="ph-focus-ring flex size-7 cursor-grab touch-none items-center justify-center rounded-[6px] text-muted-foreground transition-colors hover:bg-[color-mix(in_oklab,var(--brand-coral)_11%,transparent)] hover:text-foreground active:cursor-grabbing"
            onPointerDown={onDragStart}
            onPointerMove={onDragMove}
            onPointerUp={onDragEnd}
            onPointerCancel={onDragEnd}
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
  )
}
