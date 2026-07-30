import type { CSSProperties, ReactNode } from 'react'
import { useCallback, useLayoutEffect, useRef, useState } from 'react'
import { createPortal } from 'react-dom'

import {
  type FloatingPanelSizePreset,
  floatingPanelSizeStyle,
} from '@/components/floating/geometry/layout'
import {
  acquireWindowSlot,
  raiseWindowSlot,
  releaseWindowSlot,
  Z,
  type WindowSlot,
} from '@/lib/design/layering'
import { cn } from '@/lib/design/cn'

import { floatingRoot } from './floatingRoot'

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
  // WINDOW-tier panels hold a slot in the band for as long as they are mounted:
  // it opens at the front (newest above its peers) and is re-raised on any
  // pointer interaction (focus raises). The allocator re-seats live slots when
  // the band would otherwise run off its ceiling, hence the change callback —
  // a panel's z can move without that panel doing anything. The OVERLAY tier is
  // a single fixed value above the whole band and claims no slot.
  const [zIndex, setZIndex] = useState<number>(() =>
    layer === 'overlay' ? Z.OVERLAY : Z.WINDOW,
  )
  const slotRef = useRef<WindowSlot | null>(null)
  // Layout effect, not effect: the slot must be claimed before the browser
  // paints, or the panel shows for one frame at the band floor — behind the
  // peers it just opened over.
  useLayoutEffect(() => {
    if (layer === 'overlay') {
      return
    }
    const slot = acquireWindowSlot(setZIndex)
    slotRef.current = slot
    setZIndex(slot.z)
    return () => {
      releaseWindowSlot(slot)
      slotRef.current = null
    }
  }, [layer])
  const bringToFront = useCallback(() => {
    if (layer === 'overlay') return
    const slot = slotRef.current
    if (slot) {
      raiseWindowSlot(slot)
    }
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

  // Portal into the floating root so the panel escapes any ancestor that
  // establishes a backdrop root or a containing block for `fixed` (a
  // `backdrop-filter`/`transform`/`filter` ancestor — e.g. the ActionBar
  // header's `bg-chrome` in the glass theme). Without this, the panel's own
  // `backdrop-filter` frosts the ancestor's chrome instead of the page, and
  // `fixed inset-0` resolves against the ancestor rather than the viewport.
  if (typeof document === 'undefined') {
    return null
  }

  return createPortal(
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
          'pointer-events-auto relative w-full overflow-hidden rounded-[14px] border text-foreground',
          // The panel's material is the theme's, not this component's. It used
          // to hand-roll a gradient plus a 24px blur while the glass palette
          // blurred every other surface at 44px — one look, two implementations,
          // neither aware of the other.
          layer === 'overlay' ? 'surface-overlay' : 'surface-floating',
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
    </div>,
    floatingRoot(),
  )
}
