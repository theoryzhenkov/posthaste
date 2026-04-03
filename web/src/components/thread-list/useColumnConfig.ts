import { useCallback, useSyncExternalStore } from "react";
import { type ColumnId, DEFAULT_COLUMNS, ALL_COLUMNS } from "./columns";

const STORAGE_KEY = "posthaste-thread-columns";

const validIds = new Set<string>(ALL_COLUMNS);

function readFromStorage(): ColumnId[] {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return DEFAULT_COLUMNS;
    const parsed: unknown = JSON.parse(raw);
    if (!Array.isArray(parsed)) return DEFAULT_COLUMNS;
    const filtered = parsed.filter(
      (id): id is ColumnId => typeof id === "string" && validIds.has(id),
    );
    return filtered.length > 0 ? filtered : DEFAULT_COLUMNS;
  } catch {
    return DEFAULT_COLUMNS;
  }
}

let cached: ColumnId[] = readFromStorage();
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

function getSnapshot(): ColumnId[] {
  return cached;
}

function persist(columns: ColumnId[]) {
  cached = columns;
  localStorage.setItem(STORAGE_KEY, JSON.stringify(columns));
  notify();
}

export function useColumnConfig() {
  const columns = useSyncExternalStore(subscribe, getSnapshot);

  const toggleColumn = useCallback((columnId: ColumnId) => {
    const current = getSnapshot();
    if (current.includes(columnId)) {
      if (current.length <= 1) return;
      persist(current.filter((id) => id !== columnId));
    } else {
      persist([...current, columnId]);
    }
  }, []);

  const resetColumns = useCallback(() => {
    persist([...DEFAULT_COLUMNS]);
  }, []);

  return { columns, toggleColumn, resetColumns } as const;
}
