/**
 * Vertical drag-to-reorder primitive over `@dnd-kit`, shared by the sidebar and
 * settings lists. `SortableList` owns the DnD context and emits the reordered id
 * array; `SortableRow` makes one row draggable while leaving its inner content
 * (links/buttons) clickable — the 5px activation constraint disambiguates a
 * click from a drag.
 *
 */
import {
  DndContext,
  PointerSensor,
  closestCenter,
  useSensor,
  useSensors,
  type DragEndEvent,
} from '@dnd-kit/core'
import {
  SortableContext,
  arrayMove,
  useSortable,
  verticalListSortingStrategy,
} from '@dnd-kit/sortable'
import { CSS } from '@dnd-kit/utilities'
import type { ReactNode } from 'react'

import { cn } from '../../../lib/design/cn'

export function SortableList({
  ids,
  onReorder,
  children,
}: {
  ids: string[]
  /** Called with the full new order once a drag settles. */
  onReorder: (orderedIds: string[]) => void
  children: ReactNode
}) {
  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 5 } }),
  )

  function handleDragEnd(event: DragEndEvent) {
    const { active, over } = event
    if (!over || active.id === over.id) {
      return
    }
    const oldIndex = ids.indexOf(String(active.id))
    const newIndex = ids.indexOf(String(over.id))
    if (oldIndex < 0 || newIndex < 0) {
      return
    }
    onReorder(arrayMove(ids, oldIndex, newIndex))
  }

  return (
    <DndContext
      sensors={sensors}
      collisionDetection={closestCenter}
      onDragEnd={handleDragEnd}
    >
      <SortableContext items={ids} strategy={verticalListSortingStrategy}>
        {children}
      </SortableContext>
    </DndContext>
  )
}

export function SortableRow({
  id,
  children,
  className,
}: {
  id: string
  children: ReactNode
  className?: string
}) {
  const { setNodeRef, transform, transition, listeners, isDragging } =
    useSortable({ id })
  const style = {
    transform: CSS.Transform.toString(transform),
    transition,
  }
  // Only the pointer `listeners` are spread (not the button-role `attributes`),
  // so the wrapper stays a plain element and the inner row keeps its own
  // semantics. Pointer drag works for everyone; keyboard reorder is a follow-up.
  return (
    <div
      ref={setNodeRef}
      style={style}
      className={cn(isDragging && 'relative z-10 opacity-70', className)}
      {...listeners}
    >
      {children}
    </div>
  )
}
