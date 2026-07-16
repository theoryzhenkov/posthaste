import {
  useRef,
  useState,
  type Dispatch,
  type RefObject,
  type SetStateAction,
  type PointerEvent as ReactPointerEvent,
} from 'react'

import {
  type ActiveRails,
  type GuideLayout,
  type PanelOffset,
  guideColumns,
  guideRows,
  resistPanelOffset,
} from '@/floatingPanelGeometry'

import { HEADER_NO_DRAG_SELECTOR } from './constants'
import { panelGeometry, persistPanelOffset, viewportSize } from './geometry'

export function usePanelDrag({
  isExpanded,
  panelOffset,
  panelRef,
  setPanelOffset,
  storageKey,
}: {
  isExpanded: boolean
  panelOffset: PanelOffset
  panelRef: RefObject<HTMLDivElement | null>
  setPanelOffset: Dispatch<SetStateAction<PanelOffset>>
  storageKey: string
}) {
  const [isDragging, setIsDragging] = useState(false)
  const [activeRails, setActiveRails] = useState<ActiveRails>({
    x: null,
    y: null,
  })
  const [guideLayout, setGuideLayout] = useState<GuideLayout | null>(null)
  const dragRef = useRef<{
    lockedX?: number | null
    lockedY?: number | null
    pointerId: number
    startX: number
    startY: number
    originX: number
    originY: number
  } | null>(null)

  function handleHeaderPointerDown(event: ReactPointerEvent<HTMLDivElement>) {
    if (
      event.target instanceof Element &&
      event.target.closest(HEADER_NO_DRAG_SELECTOR)
    ) {
      return
    }
    handleDragStart(event)
  }

  function handleDragStart(event: ReactPointerEvent<HTMLElement>) {
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

  function handleDragMove(event: ReactPointerEvent<HTMLElement>) {
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

  function handleDragEnd(event: ReactPointerEvent<HTMLElement>) {
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

  return {
    activeRails,
    guideLayout,
    handleDragEnd,
    handleDragMove,
    handleDragStart,
    handleHeaderPointerDown,
    isDragging,
  }
}
