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
  horizontalListSortingStrategy,
} from '@dnd-kit/sortable'
import { ColumnPickerMenu } from './ColumnPickerMenu'
import { SortableColumnHeader } from './SortableColumnHeader'
import {
  SORTABLE_COLUMNS,
  type ColumnId,
  type ColumnWidths,
  type SortConfig,
  type ThreadListLayout,
  getColumnBasis,
  getColumnDef,
} from './columns'

interface ThreadListHeaderProps {
  columns: ColumnId[]
  layout: ThreadListLayout
  sort: SortConfig
  widths: ColumnWidths
  onResetColumns: () => void
  onResizeColumn: (columnId: ColumnId, width: number) => void
  onReorderColumns: (columns: ColumnId[]) => void
  onToggleColumn: (columnId: ColumnId) => void
  onToggleSort: (columnId: ColumnId) => void
}

export function ThreadListHeader({
  columns,
  layout,
  sort,
  widths,
  onResetColumns,
  onResizeColumn,
  onReorderColumns,
  onToggleColumn,
  onToggleSort,
}: ThreadListHeaderProps) {
  const dndSensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 5 } }),
  )

  function handleColumnDragEnd(event: DragEndEvent) {
    const { active, over } = event
    if (!over || active.id === over.id) {
      return
    }

    const oldIndex = columns.indexOf(active.id as ColumnId)
    const newIndex = columns.indexOf(over.id as ColumnId)
    onReorderColumns(arrayMove(columns, oldIndex, newIndex))
  }

  return (
    <ColumnPickerMenu
      activeColumns={columns}
      onToggle={onToggleColumn}
      onReset={onResetColumns}
    >
      <div
        className="grid h-[26px] items-center gap-0 px-0 font-mono text-[11px] font-semibold uppercase tracking-[0.06em] text-muted-foreground"
        style={layout.gridStyle}
      >
        <DndContext
          sensors={dndSensors}
          collisionDetection={closestCenter}
          onDragEnd={handleColumnDragEnd}
        >
          <SortableContext
            items={columns}
            strategy={horizontalListSortingStrategy}
          >
            {columns.map((colId) => {
              const def = getColumnDef(colId)
              const isFirstColumn = colId === columns[0]
              const isLastColumn = colId === columns[columns.length - 1]
              const isSortable = SORTABLE_COLUMNS.has(colId)
              const canResize = def.resizable === true
              return (
                <SortableColumnHeader
                  key={colId}
                  id={colId}
                  label={def.label}
                  icon={def.header}
                  align={def.align}
                  isSortable={isSortable}
                  resizeBasis={
                    canResize ? getColumnBasis(colId, widths) : undefined
                  }
                  resizeMinWidth={def.minWidth ?? def.basis}
                  sortDirection={
                    sort.columnId === colId ? sort.direction : undefined
                  }
                  showResizeDivider={!isLastColumn}
                  resizePlacement={isLastColumn ? 'end-edge' : 'interior'}
                  showStartResizeHandle={canResize && isFirstColumn}
                  onSort={() => onToggleSort(colId)}
                  onResize={
                    canResize
                      ? (width) => onResizeColumn(colId, width)
                      : undefined
                  }
                />
              )
            })}
          </SortableContext>
        </DndContext>
      </div>
    </ColumnPickerMenu>
  )
}
