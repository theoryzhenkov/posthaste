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
 * The list's entry keeps layout with it — sort and per-column widths — since
 * those are meaningless to a stacked row. The detail pane stores a visible
 * set and nothing else.
 */
import { useCallback } from 'react'
import type { SortDirection } from '@/domain/vocabulary'
import { createStoredStore, useStore } from '@/lib/store'

import {
  fieldsForSurface,
  isMessageFieldId,
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

const STORAGE_KEY = 'posthaste-message-columns-v7'

/**
 * The detail rows shown out of the box: the recipients a reader expects to
 * see on every message. `cc`, `bcc` and `replyTo` are available but OFF by
 * default — they are absent on most mail, and a reader who wants them can
 * turn them on. An enabled-but-absent field renders nothing at all.
 */
const DEFAULT_DETAIL_FIELDS: MessageFieldId[] = ['to']

interface StoredConfig {
  columns: ColumnId[]
  sort: SortConfig
  widths: ColumnWidths
  detailFields: MessageFieldId[]
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
    const detailFields = Array.isArray(obj.detailFields)
      ? obj.detailFields.filter(isValidDetailFieldId)
      : DEFAULT_CONFIG.detailFields

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
 * The detail pane's chosen rows — the same store and the same storage entry
 * the column picker writes, so the two selections stay one saved preference.
 *
 * Selection order is deliberately NOT stored: rows render in the registry's
 * declaration order, so `To` stays above `CC` however they were toggled on.
 * Unlike columns there is no minimum — choosing to show no rows is allowed.
 */
export function useDetailFieldConfig() {
  const config = useStore(columnConfigStore)

  const toggleDetailField = useCallback((fieldId: MessageFieldId) => {
    const { detailFields } = currentConfig()
    persist({
      detailFields: detailFields.includes(fieldId)
        ? detailFields.filter((id) => id !== fieldId)
        : [...detailFields, fieldId],
    })
  }, [])

  const resetDetailFields = useCallback(() => {
    persist({ detailFields: [...DEFAULT_DETAIL_FIELDS] })
  }, [])

  return {
    detailFields: config.detailFields,
    toggleDetailField,
    resetDetailFields,
  } as const
}
