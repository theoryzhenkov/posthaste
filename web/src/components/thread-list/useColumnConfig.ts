import { useCallback, useSyncExternalStore } from "react";
import {
  type ColumnId,
  type ColumnWidths,
  type SortConfig,
  type SortDirection,
  ALL_COLUMNS,
  DEFAULT_COLUMNS,
  DEFAULT_SORT,
  SORTABLE_COLUMNS,
  getColumnDef,
} from "./columns";

const STORAGE_KEY = "posthaste-thread-columns-v6";

interface StoredConfig {
  columns: ColumnId[];
  sort: SortConfig;
  widths: ColumnWidths;
}

const DEFAULT_CONFIG: StoredConfig = {
  columns: [...DEFAULT_COLUMNS],
  sort: { ...DEFAULT_SORT },
  widths: {},
};

const validIds = new Set<string>(ALL_COLUMNS);

function isValidColumnId(id: unknown): id is ColumnId {
  return typeof id === "string" && validIds.has(id);
}

function readFromStorage(): StoredConfig {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return DEFAULT_CONFIG;
    const parsed: unknown = JSON.parse(raw);

    // Migrate from old format (plain array of column IDs)
    if (Array.isArray(parsed)) {
      const columns = parsed.filter(isValidColumnId);
      const migrated: StoredConfig = {
        columns: columns.length > 0 ? columns : DEFAULT_CONFIG.columns,
        sort: DEFAULT_CONFIG.sort,
        widths: {},
      };
      localStorage.setItem(STORAGE_KEY, JSON.stringify(migrated));
      return migrated;
    }

    if (typeof parsed !== "object" || parsed === null) return DEFAULT_CONFIG;
    const obj = parsed as Record<string, unknown>;

    let columns = DEFAULT_CONFIG.columns;
    if (Array.isArray(obj.columns)) {
      const filtered = obj.columns.filter(isValidColumnId);
      if (filtered.length > 0) columns = filtered;
    }

    let sort = DEFAULT_CONFIG.sort;
    if (typeof obj.sort === "object" && obj.sort !== null) {
      const s = obj.sort as Record<string, unknown>;
      if (
        isValidColumnId(s.columnId) &&
        (s.direction === "asc" || s.direction === "desc")
      ) {
        sort = {
          columnId: s.columnId,
          direction: s.direction as SortDirection,
        };
      }
    }

    const widths: ColumnWidths = {};
    if (typeof obj.widths === "object" && obj.widths !== null && !Array.isArray(obj.widths)) {
      const w = obj.widths as Record<string, unknown>;
      for (const [key, val] of Object.entries(w)) {
        if (isValidColumnId(key) && typeof val === "number" && val > 0) {
          const def = getColumnDef(key);
          if (def.resizable === true) {
            widths[key] = Math.max(def.minWidth ?? def.basis, Math.round(val));
          }
        }
      }
    }

    return { columns, sort, widths };
  } catch {
    return DEFAULT_CONFIG;
  }
}

let cached: StoredConfig = readFromStorage();
const listeners = new Set<() => void>();

function notify() {
  for (const fn of listeners) fn();
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

function getSnapshot(): StoredConfig {
  return cached;
}

function persist(config: StoredConfig) {
  cached = config;
  localStorage.setItem(STORAGE_KEY, JSON.stringify(config));
  notify();
}

export function useColumnConfig() {
  const config = useSyncExternalStore(subscribe, getSnapshot);

  const toggleColumn = useCallback((columnId: ColumnId) => {
    const { columns, sort, widths } = getSnapshot();
    if (columns.includes(columnId)) {
      if (columns.length <= 1) return;
      const rest = { ...widths };
      delete rest[columnId];
      persist({ columns: columns.filter((id) => id !== columnId), sort, widths: rest });
    } else {
      persist({ columns: [...columns, columnId], sort, widths });
    }
  }, []);

  const reorderColumns = useCallback((newColumns: ColumnId[]) => {
    const { sort, widths } = getSnapshot();
    persist({ columns: newColumns, sort, widths });
  }, []);

  const resetColumns = useCallback(() => {
    persist({ columns: [...DEFAULT_COLUMNS], sort: { ...DEFAULT_SORT }, widths: {} });
  }, []);

  const setColumnWidth = useCallback((columnId: ColumnId, width: number) => {
    const def = getColumnDef(columnId);
    if (def.resizable !== true) {
      return;
    }
    const { columns, sort, widths } = getSnapshot();
    const nextWidth = Math.max(def.minWidth ?? def.basis, Math.round(width));
    persist({ columns, sort, widths: { ...widths, [columnId]: nextWidth } });
  }, []);

  const toggleSort = useCallback((columnId: ColumnId) => {
    if (!SORTABLE_COLUMNS.has(columnId)) return;
    const { columns, sort, widths } = getSnapshot();
    if (sort.columnId === columnId) {
      persist({
        columns,
        sort: {
          columnId,
          direction: sort.direction === "asc" ? "desc" : "asc",
        },
        widths,
      });
    } else {
      const direction: SortDirection =
        columnId === "date" ? "desc" : "asc";
      persist({ columns, sort: { columnId, direction }, widths });
    }
  }, []);

  return {
    columns: config.columns,
    sort: config.sort,
    widths: config.widths,
    toggleColumn,
    reorderColumns,
    resetColumns,
    toggleSort,
    setColumnWidth,
  } as const;
}
