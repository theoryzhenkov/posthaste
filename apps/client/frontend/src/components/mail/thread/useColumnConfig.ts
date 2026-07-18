/**
 * Message-table column layout — visible set, sort, and per-column widths —
 * persisted to localStorage as one config. A `createStoredStore` (R5) shared
 * by every table instance.
 */
import { useCallback } from 'react'
import type { SortDirection } from '@/domain/vocabulary'
import { createStoredStore, useStore } from '@/lib/store'

import {
  type ColumnId,
  type ColumnWidths,
  type SortConfig,
  ALL_COLUMNS,
  DEFAULT_COLUMNS,
  DEFAULT_SORT,
  SORTABLE_COLUMNS,
  getColumnDef,
} from './columns'

const STORAGE_KEY = 'posthaste-message-columns-v7'

interface StoredConfig {
  columns: ColumnId[]
  sort: SortConfig
  widths: ColumnWidths
}

const DEFAULT_CONFIG: StoredConfig = {
  columns: [...DEFAULT_COLUMNS],
  sort: { ...DEFAULT_SORT },
  widths: {},
}

const validIds = new Set<string>(ALL_COLUMNS)

function isValidColumnId(id: unknown): id is ColumnId {
  return typeof id === 'string' && validIds.has(id)
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

    return { columns, sort, widths }
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

function persist(config: StoredConfig) {
  columnConfigStore.set(config)
}

export function useColumnConfig() {
  const config = useStore(columnConfigStore)

  const toggleColumn = useCallback((columnId: ColumnId) => {
    const { columns, sort, widths } = currentConfig()
    if (columns.includes(columnId)) {
      if (columns.length <= 1) return
      const rest = { ...widths }
      delete rest[columnId]
      persist({
        columns: columns.filter((id) => id !== columnId),
        sort,
        widths: rest,
      })
    } else {
      persist({ columns: [...columns, columnId], sort, widths })
    }
  }, [])

  const reorderColumns = useCallback((newColumns: ColumnId[]) => {
    const { sort, widths } = currentConfig()
    persist({ columns: newColumns, sort, widths })
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
    const { columns, sort, widths } = currentConfig()
    const nextWidth = Math.max(def.minWidth ?? def.basis, Math.round(width))
    persist({ columns, sort, widths: { ...widths, [columnId]: nextWidth } })
  }, [])

  const toggleSort = useCallback((columnId: ColumnId) => {
    if (!SORTABLE_COLUMNS.has(columnId)) return
    const { columns, sort, widths } = currentConfig()
    if (sort.columnId === columnId) {
      persist({
        columns,
        sort: {
          columnId,
          direction: sort.direction === 'asc' ? 'desc' : 'asc',
        },
        widths,
      })
    } else {
      const direction: SortDirection = columnId === 'date' ? 'desc' : 'asc'
      persist({ columns, sort: { columnId, direction }, widths })
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
