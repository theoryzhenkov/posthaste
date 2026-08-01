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
import {
  FieldPickerButton,
  FieldPickerMenu,
  fieldPickerOptions,
} from './fieldPicker'
import { SortableColumnHeader } from './SortableColumnHeader'
import {
  ALL_COLUMNS,
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

  // The last column standing cannot be turned off — the table needs one to lay
  // out — so the picker shows it as disabled rather than as a click that does
  // nothing.
  const options = fieldPickerOptions(
    ALL_COLUMNS,
    columns,
    columns.length === 1 ? columns[0] : null,
  )

  return (
    <FieldPickerMenu
      options={options}
      onToggle={onToggleColumn}
      onReset={onResetColumns}
    >
      <div className="relative">
        <div
          className="grid h-[26px] items-center gap-0 px-0 font-mono text-meta font-semibold uppercase tracking-[0.06em] text-muted-foreground"
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
        {/* At the header's right end, so the row carries its own affordance:
            the thing being configured is the thing with the button on it.
            Inset by the width of the last column's end-edge resize handle
            (`w-4`, and z-20 above this), which must stay grabbable — the
            button covers a little of that column's label instead, where the
            only loss is a click that would have re-sorted. */}
        <FieldPickerButton
          className="absolute right-4 top-0 h-[26px] rounded-none bg-[var(--list-header)] text-muted-foreground"
          label="Choose columns"
          options={options}
          onToggle={onToggleColumn}
          onReset={onResetColumns}
        />
      </div>
    </FieldPickerMenu>
  )
}
