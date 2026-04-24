import { useCallback, useRef } from 'react'

export type ColumnResizeHandlePlacement = 'interior' | 'start-edge' | 'end-edge'

interface ColumnResizeHandleProps {
  onResize: (width: number) => void
  basis: number
  minWidth?: number
  showDivider?: boolean
  placement?: ColumnResizeHandlePlacement
}

export function ColumnResizeHandle({
  onResize,
  basis,
  minWidth = 32,
  showDivider = true,
  placement = 'interior',
}: ColumnResizeHandleProps) {
  const draggingRef = useRef(false)
  const isStartEdge = placement === 'start-edge'

  const handlePointerDown = useCallback(
    (e: React.PointerEvent) => {
      e.preventDefault()
      e.stopPropagation()

      const startX = e.clientX
      const startWidth = basis
      draggingRef.current = true

      const target = e.currentTarget as HTMLElement
      target.setPointerCapture(e.pointerId)

      function onPointerMove(ev: PointerEvent) {
        if (!draggingRef.current) return
        const delta = ev.clientX - startX
        const nextDelta = isStartEdge ? -delta : delta
        const newWidth = Math.max(minWidth, startWidth + nextDelta)
        onResize(newWidth)
      }

      function onPointerUp() {
        draggingRef.current = false
        target.removeEventListener('pointermove', onPointerMove)
        target.removeEventListener('pointerup', onPointerUp)
      }

      target.addEventListener('pointermove', onPointerMove)
      target.addEventListener('pointerup', onPointerUp)
    },
    [basis, isStartEdge, minWidth, onResize],
  )

  return (
    <div
      className={
        placement === 'start-edge'
          ? 'group absolute left-0 top-0 z-20 flex h-full w-4 cursor-col-resize items-center justify-start'
          : placement === 'end-edge'
            ? 'group absolute right-0 top-0 z-20 flex h-full w-4 cursor-col-resize items-center justify-end'
            : 'group absolute right-0 top-0 z-20 flex h-full w-2 translate-x-1/2 cursor-col-resize items-center justify-center'
      }
      onPointerDown={handlePointerDown}
      onClick={(e) => e.stopPropagation()}
    >
      <div
        className={
          showDivider
            ? 'h-full w-px bg-border/80 transition-colors group-hover:bg-brand-coral/70 group-active:bg-brand-coral'
            : 'h-full w-px bg-transparent transition-colors group-hover:bg-brand-coral/70 group-active:bg-brand-coral'
        }
      />
    </div>
  )
}
