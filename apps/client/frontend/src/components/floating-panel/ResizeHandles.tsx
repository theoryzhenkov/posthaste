import type { PointerEvent, PointerEventHandler } from 'react'

import type { ResizeHandle } from '@/floatingPanelGeometry'
import { cn } from '@/lib/utils'

import { RESIZE_HANDLES } from './constants'

export function ResizeHandles({
  isExpanded,
  onResizeEnd,
  onResizeMove,
  onResizeStart,
}: {
  isExpanded: boolean
  onResizeEnd: PointerEventHandler<HTMLDivElement>
  onResizeMove: PointerEventHandler<HTMLDivElement>
  onResizeStart: (
    handle: ResizeHandle,
    event: PointerEvent<HTMLDivElement>,
  ) => void
}) {
  if (isExpanded) {
    return null
  }

  return (
    <>
      {RESIZE_HANDLES.map(({ handle, className, corner }) => (
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
          onPointerDown={(event) => onResizeStart(handle, event)}
          onPointerMove={onResizeMove}
          onPointerUp={onResizeEnd}
          onPointerCancel={onResizeEnd}
        />
      ))}
    </>
  )
}
