import type { CSSProperties, ReactNode } from 'react'
import { useCallback, useState } from 'react'

import {
  type FloatingPanelSizePreset,
  floatingPanelSizeStyle,
} from '@/components/floating/geometry/layout'
import { nextWindowZIndex, Z } from '@/lib/design/layering'
import { cn } from '@/lib/design/cn'

import { FloatingPanelGuides } from './panel/FloatingPanelGuides'
import { FloatingPanelHeader } from './panel/FloatingPanelHeader'
import { ResizeCellBadge } from './panel/ResizeCellBadge'
import { ResizeHandles } from './panel/ResizeHandles'
import { usePanelDismissal } from './hooks/usePanelDismissal'
import { usePanelDrag } from './hooks/usePanelDrag'
import { usePanelPlacement } from './hooks/usePanelPlacement'
import { usePanelResize } from './hooks/usePanelResize'

interface FloatingPanelProps {
  children: ReactNode
  className?: string
  closeIgnoreSelector?: string
  header: ReactNode
  headerClassName?: string
  /**
   * Which layering tier this panel lives in.
   *  - `'window'` (default): a peer window in the WINDOW band. Multiple such
   *    panels bring-to-front on open/focus (last-touched on top) while staying
   *    bounded below the OVERLAY tier.
   *  - `'overlay'`: a global overlay (the command palette) pinned above all
   *    windows but below dialogs/toasts. Does not participate in the band.
   */
  layer?: 'window' | 'overlay'
  panelLabel: string
  sizePreset?: FloatingPanelSizePreset
  storageKey: string
  onClose: () => void
  onOpenInWindow?: () => void
}

export function FloatingPanel({
  children,
  className,
  closeIgnoreSelector,
  header,
  headerClassName,
  layer = 'window',
  panelLabel,
  sizePreset,
  storageKey,
  onClose,
  onOpenInWindow,
}: FloatingPanelProps) {
  // WINDOW-tier panels claim a fresh z within the band on mount (newest above
  // its peers) and re-claim it on any pointer interaction (focus raises). The
  // OVERLAY tier is a single fixed value above the whole band.
  const [zIndex, setZIndex] = useState(() =>
    layer === 'overlay' ? Z.OVERLAY : nextWindowZIndex(),
  )
  const bringToFront = useCallback(() => {
    if (layer === 'overlay') return
    setZIndex(nextWindowZIndex())
  }, [layer])
  const [isPinned, setIsPinned] = useState(false)
  const [isExpanded, setIsExpanded] = useState(false)
  const {
    panelOffset,
    panelRef,
    panelSize,
    setPanelOffset,
    setPanelSize,
    sizeStorageKey,
  } = usePanelPlacement({ sizePreset, storageKey })
  const drag = usePanelDrag({
    isExpanded,
    panelOffset,
    panelRef,
    setPanelOffset,
    storageKey,
  })
  const resize = usePanelResize({
    isExpanded,
    panelRef,
    setPanelOffset,
    setPanelSize,
    sizePreset,
    sizeStorageKey,
    storageKey,
  })
  usePanelDismissal({
    closeIgnoreSelector,
    isPinned,
    onClose,
    panelRef,
  })

  const floatingSizeStyle: CSSProperties =
    !isExpanded && sizePreset ? floatingPanelSizeStyle(sizePreset) : {}
  const resizeSizeStyle: CSSProperties =
    !isExpanded && panelSize
      ? {
          width: `${panelSize.width}px`,
          height: `${panelSize.height}px`,
        }
      : {}

  return (
    <div
      className={cn(
        'pointer-events-none fixed inset-0 flex items-start justify-center px-4',
        isExpanded ? 'pt-4' : 'pt-[54px]',
      )}
      style={{ zIndex }}
      aria-live="polite"
    >
      <FloatingPanelGuides
        activeRails={drag.activeRails}
        activeResizeLines={resize.activeResizeLines}
        guideLayout={drag.guideLayout}
        isDragging={drag.isDragging}
        isResizing={resize.isResizing}
        resizeHandle={resize.resizeHandle}
        resizeLines={resize.resizeLines}
      />
      <div
        ref={panelRef}
        onPointerDownCapture={bringToFront}
        className={cn(
          'pointer-events-auto relative w-full overflow-hidden rounded-[14px] border [border-color:color-mix(in_oklab,var(--brand-coral)_22%,var(--border))] bg-[linear-gradient(135deg,color-mix(in_oklab,var(--brand-coral)_14%,var(--panel))_0%,color-mix(in_oklab,var(--ring)_7%,var(--panel))_50%,var(--panel)_100%)] text-foreground shadow-[0_28px_80px_rgb(0_0_0/0.24)] backdrop-blur-[24px] backdrop-saturate-150 dark:shadow-[0_28px_80px_rgb(0_0_0/0.48)]',
          className,
          isExpanded
            ? 'h-[calc(100vh-2rem)] max-h-[calc(100vh-2rem)] max-w-[calc(100vw-2rem)]'
            : 'max-h-[calc(100vh-2rem)] max-w-[calc(100vw-2rem)] min-h-48 min-w-72',
          // Animate double-click grow / restore, but never during a live drag or
          // resize (that must track the pointer instantly).
          !isExpanded &&
            !drag.isDragging &&
            !resize.isResizing &&
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
        <FloatingPanelHeader
          header={header}
          headerClassName={headerClassName}
          isExpanded={isExpanded}
          isPinned={isPinned}
          panelLabel={panelLabel}
          setIsExpanded={setIsExpanded}
          setIsPinned={setIsPinned}
          onClose={onClose}
          onDragEnd={drag.handleDragEnd}
          onDragMove={drag.handleDragMove}
          onDragStart={drag.handleDragStart}
          onHeaderPointerDown={drag.handleHeaderPointerDown}
          onOpenInWindow={onOpenInWindow}
        />
        {children}
        <ResizeHandles
          isExpanded={isExpanded}
          onResizeEnd={resize.handleResizeEnd}
          onResizeMove={resize.handleResizeMove}
          onResizeStart={resize.handleResizeStart}
        />
        <ResizeCellBadge isResizing={resize.isResizing} panelSize={panelSize} />
      </div>
    </div>
  )
}
