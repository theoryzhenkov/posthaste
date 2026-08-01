/**
 * Which message fields each surface shows, and how — persisted to
 * localStorage as one config, via a `createStoredStore` (R5) shared by every
 * consumer.
 *
 * Both surfaces live here on purpose. They are one user-facing idea ("choose
 * what a message shows"), they are driven by one registry, and giving the
 * detail pane its own store would mean a second storage key with its own
 * validation and its own migration to keep in step.
 *
 * Each surface stores what layout means for it, and they do not match: the
 * list keeps sort and per-column widths, which mean nothing to a stacked row,
 * while the detail pane keeps an ORDERED list of rows carrying each one's
 * emphasis and whether it prints its label. Order is the array's, because the
 * reader arranges those rows themselves.
 */
import { useCallback } from 'react'
import type { SortDirection } from '@/domain/vocabulary'
import { createStoredStore, useStore } from '@/lib/store'

import {
  detailFieldDefault,
  fieldsForSurface,
  isMessageFieldEmphasis,
  isMessageFieldId,
  type DetailFieldSetting,
  type MessageFieldId,
} from '../fields'
import {
  type ColumnId,
  type ColumnWidths,
  type SortConfig,
  DEFAULT_COLUMNS,
  DEFAULT_SORT,
  SORTABLE_COLUMNS,
  getColumnDef,
} from './columns'

/**
 * The key does NOT move for the detail-row rework, even though the shape of
 * `detailFields` does. A stored config is a reader's arrangement — column
 * widths, sort, chosen rows — and throwing it away to dodge a shape change
 * would be the loudest possible way to handle a change they never asked
 * about. `readDetailFields` upgrades the old shape instead.
 */
const STORAGE_KEY = 'posthaste-message-columns-v7'

/**
 * The detail rows shown out of the box, in order.
 *
 * `subject`, `from` and `tags` are here because nothing else draws them any
 * more: the header's heading, byline and chip row all became ordinary fields,
 * and a default omitting one would silently delete it from the UI for every
 * reader who never opens the picker.
 *
 * `cc` and `bcc` are here because they cost nothing when absent — an
 * enabled-but-empty field renders no row at all — and being CC'd rather than
 * addressed is worth seeing. `bcc` is stripped in transit on received mail,
 * so in practice it appears only on the user's own sent mail and drafts,
 * which is exactly where it means something.
 *
 * `replyTo` and `source` stay off: the first usually just repeats the sender
 * (the case where it does not is the case worth opting into), and the second
 * is only interesting to a reader with several accounts.
 *
 * Each entry's emphasis and label come from the field's own declaration, so
 * this list says only WHICH rows and in what order.
 */
const DEFAULT_DETAIL_FIELDS: DetailFieldSetting[] = (
  ['subject', 'from', 'to', 'cc', 'bcc', 'tags'] as const
).map(detailFieldDefault)

/**
 * The rows that had no home outside the header before it became these fields.
 * A stored selection written before then cannot name them, so migrating one
 * faithfully would produce a message with no subject, no sender and no tags —
 * which is why the upgrade adds these back rather than trusting their absence.
 */
const STRUCTURAL_DETAIL_FIELDS: MessageFieldId[] = ['subject', 'from', 'tags']

interface StoredConfig {
  columns: ColumnId[]
  sort: SortConfig
  widths: ColumnWidths
  detailFields: DetailFieldSetting[]
}

const DEFAULT_CONFIG: StoredConfig = {
  columns: [...DEFAULT_COLUMNS],
  sort: { ...DEFAULT_SORT },
  widths: {},
  detailFields: [...DEFAULT_DETAIL_FIELDS],
}

const columnIds = new Set<string>(fieldsForSurface('list'))
const detailIds = new Set<string>(fieldsForSurface('detail'))

function isValidColumnId(id: unknown): id is ColumnId {
  return typeof id === 'string' && columnIds.has(id)
}

function isValidDetailFieldId(id: unknown): id is MessageFieldId {
  return isMessageFieldId(id) && detailIds.has(id)
}

/**
 * Reads the detail selection out of storage, in either shape it can have.
 *
 * Before the header became its fields, a selection was a flat list of ids and
 * carried nothing else; now each row also holds its emphasis and whether it
 * prints its label, and the ARRAY ORDER is the reader's row order. Both shapes
 * arrive here:
 *
 * - a list of ids (pre-rework) is upgraded entry by entry to the declared
 *   presentation of each field, then has the structural rows it could not have
 *   named added at the front — see `STRUCTURAL_DETAIL_FIELDS`;
 * - a list of entries is taken as written, with each part validated on its own
 *   so one bad emphasis costs that field its emphasis and nothing else.
 *
 * Anything else — a number, a string, a missing key — returns `null` and the
 * caller falls back to the defaults. Junk resets; a real arrangement never
 * does.
 *
 * Exported for its own tests: the store it feeds is module-scoped and reads
 * storage once at import, so the upgrade path is unreachable through the hook.
 */
export function readDetailFields(value: unknown): DetailFieldSetting[] | null {
  if (!Array.isArray(value)) return null

  // An EMPTY array is not evidence of either shape, and it is read as the new
  // one: "show no rows" is a choice the settings editor now offers, and a
  // reader who makes it must not find the header repopulated on next launch.
  // The cost is a pre-rework reader who had turned their one row off keeping
  // an empty header — a deliberate state then and now.
  const legacy =
    value.length > 0 && value.every((entry) => typeof entry === 'string')
  const entries: DetailFieldSetting[] = []
  const seen = new Set<MessageFieldId>()

  for (const entry of value) {
    const raw: unknown =
      typeof entry === 'string' ? { id: entry } : (entry as unknown)
    if (typeof raw !== 'object' || raw === null) continue
    const record = raw as Record<string, unknown>
    if (!isValidDetailFieldId(record.id) || seen.has(record.id)) continue

    const declared = detailFieldDefault(record.id)
    entries.push({
      id: record.id,
      emphasis: isMessageFieldEmphasis(record.emphasis)
        ? record.emphasis
        : declared.emphasis,
      showLabel:
        typeof record.showLabel === 'boolean'
          ? record.showLabel
          : declared.showLabel,
    })
    seen.add(record.id)
  }

  if (legacy) {
    const missing = STRUCTURAL_DETAIL_FIELDS.filter((id) => !seen.has(id)).map(
      detailFieldDefault,
    )
    return [...missing, ...entries]
  }

  return entries
}

/**
 * One row moved one place, or the same list back when the move would fall off
 * either end (the settings buttons disable at the ends, but a list is not
 * trusted to have been rendered by them).
 *
 * Pure and exported so the arithmetic is testable: the hook that wraps it
 * needs a store that reads storage at import, and an off-by-one here silently
 * rearranges the header of every message the reader owns.
 */
export function moveDetailRow(
  fields: DetailFieldSetting[],
  fieldId: MessageFieldId,
  direction: -1 | 1,
): DetailFieldSetting[] {
  const from = fields.findIndex((field) => field.id === fieldId)
  const to = from + direction
  if (from === -1 || to < 0 || to >= fields.length) return fields
  const next = [...fields]
  const [moved] = next.splice(from, 1)
  next.splice(to, 0, moved)
  return next
}

function readStoredConfig(raw: string | null): StoredConfig {
  if (!raw) return DEFAULT_CONFIG
  try {
    const parsed: unknown = JSON.parse(raw)

    // Migrate from old format (plain array of column IDs); the migrated shape
    // persists on the next change.
    if (Array.isArray(parsed)) {
      const columns = parsed.filter(isValidColumnId)
      return {
        columns: columns.length > 0 ? columns : DEFAULT_CONFIG.columns,
        sort: DEFAULT_CONFIG.sort,
        widths: {},
        detailFields: DEFAULT_CONFIG.detailFields,
      }
    }

    if (typeof parsed !== 'object' || parsed === null) return DEFAULT_CONFIG
    const obj = parsed as Record<string, unknown>

    let columns = DEFAULT_CONFIG.columns
    if (Array.isArray(obj.columns)) {
      const filtered = obj.columns.filter(isValidColumnId)
      if (filtered.length > 0) columns = filtered
    }

    let sort = DEFAULT_CONFIG.sort
    if (typeof obj.sort === 'object' && obj.sort !== null) {
      const s = obj.sort as Record<string, unknown>
      if (
        isValidColumnId(s.columnId) &&
        (s.direction === 'asc' || s.direction === 'desc')
      ) {
        sort = {
          columnId: s.columnId,
          direction: s.direction as SortDirection,
        }
      }
    }

    const widths: ColumnWidths = {}
    if (
      typeof obj.widths === 'object' &&
      obj.widths !== null &&
      !Array.isArray(obj.widths)
    ) {
      const w = obj.widths as Record<string, unknown>
      for (const [key, val] of Object.entries(w)) {
        if (isValidColumnId(key) && typeof val === 'number' && val > 0) {
          const def = getColumnDef(key)
          if (def.resizable === true) {
            widths[key] = Math.max(def.minWidth ?? def.basis, Math.round(val))
          }
        }
      }
    }

    // An absent key means a config stored before the detail pane had a
    // selection; an EMPTY array is a real choice (show no rows) and is kept.
    const detailFields =
      readDetailFields(obj.detailFields) ?? DEFAULT_CONFIG.detailFields

    return { columns, sort, widths, detailFields }
  } catch {
    return DEFAULT_CONFIG
  }
}

const columnConfigStore = createStoredStore<StoredConfig>({
  key: STORAGE_KEY,
  codec: { read: readStoredConfig, write: (config) => JSON.stringify(config) },
})

function currentConfig(): StoredConfig {
  return columnConfigStore.get()
}

/** Merges a patch onto the stored config. Patch rather than whole-value so a
 *  mutator that touches one surface cannot silently drop the other's key. */
function persist(patch: Partial<StoredConfig>) {
  columnConfigStore.set({ ...currentConfig(), ...patch })
}

export function useColumnConfig() {
  const config = useStore(columnConfigStore)

  const toggleColumn = useCallback((columnId: ColumnId) => {
    const { columns, widths } = currentConfig()
    if (columns.includes(columnId)) {
      // The table needs at least one column to lay out.
      if (columns.length <= 1) return
      const rest = { ...widths }
      delete rest[columnId]
      persist({ columns: columns.filter((id) => id !== columnId), widths: rest })
    } else {
      persist({ columns: [...columns, columnId] })
    }
  }, [])

  const reorderColumns = useCallback((newColumns: ColumnId[]) => {
    persist({ columns: newColumns })
  }, [])

  const resetColumns = useCallback(() => {
    persist({
      columns: [...DEFAULT_COLUMNS],
      sort: { ...DEFAULT_SORT },
      widths: {},
    })
  }, [])

  const setColumnWidth = useCallback((columnId: ColumnId, width: number) => {
    const def = getColumnDef(columnId)
    if (def.resizable !== true) {
      return
    }
    const { widths } = currentConfig()
    const nextWidth = Math.max(def.minWidth ?? def.basis, Math.round(width))
    persist({ widths: { ...widths, [columnId]: nextWidth } })
  }, [])

  const toggleSort = useCallback((columnId: ColumnId) => {
    if (!SORTABLE_COLUMNS.has(columnId)) return
    const { sort } = currentConfig()
    if (sort.columnId === columnId) {
      persist({
        sort: {
          columnId,
          direction: sort.direction === 'asc' ? 'desc' : 'asc',
        },
      })
    } else {
      const direction: SortDirection = columnId === 'date' ? 'desc' : 'asc'
      persist({ sort: { columnId, direction } })
    }
  }, [])

  return {
    columns: config.columns,
    sort: config.sort,
    widths: config.widths,
    toggleColumn,
    reorderColumns,
    resetColumns,
    toggleSort,
    setColumnWidth,
  } as const
}

/**
 * The detail pane's rows — which fields, in what order, and how each one
 * presents. Same store and same storage entry as the column picker, so the two
 * surfaces stay one saved preference.
 *
 * The array's ORDER is the header's row order: unlike columns, whose order the
 * reader drags in place, detail rows are reordered from settings, because the
 * reading pane is content and dragging content around is a different promise.
 * Unlike columns there is also no minimum — showing no rows is allowed.
 *
 * A newly enabled field arrives with its DECLARED presentation and lands at
 * the end, where the reader put their attention when they turned it on.
 */
export function useDetailFieldConfig() {
  const config = useStore(columnConfigStore)

  const toggleDetailField = useCallback((fieldId: MessageFieldId) => {
    const { detailFields } = currentConfig()
    persist({
      detailFields: detailFields.some((field) => field.id === fieldId)
        ? detailFields.filter((field) => field.id !== fieldId)
        : [...detailFields, detailFieldDefault(fieldId)],
    })
  }, [])

  /** Rewrites one row's presentation, leaving its place and every other row
   *  alone. */
  const updateDetailField = useCallback(
    (fieldId: MessageFieldId, patch: Partial<Omit<DetailFieldSetting, 'id'>>) => {
      const { detailFields } = currentConfig()
      persist({
        detailFields: detailFields.map((field) =>
          field.id === fieldId ? { ...field, ...patch } : field,
        ),
      })
    },
    [],
  )

  /**
   * Moves a row one place up or down. One step at a time rather than a
   * to-index move because that is the whole of what the settings controls
   * offer, and a step is the operation a reader can undo by pressing the
   * other button.
   */
  const moveDetailField = useCallback(
    (fieldId: MessageFieldId, direction: -1 | 1) => {
      const { detailFields } = currentConfig()
      persist({ detailFields: moveDetailRow(detailFields, fieldId, direction) })
    },
    [],
  )

  const resetDetailFields = useCallback(() => {
    persist({ detailFields: [...DEFAULT_DETAIL_FIELDS] })
  }, [])

  return {
    detailFields: config.detailFields,
    toggleDetailField,
    updateDetailField,
    moveDetailField,
    resetDetailFields,
  } as const
}
