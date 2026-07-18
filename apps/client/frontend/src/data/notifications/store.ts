/**
 * App-wide notification center.
 *
 * A module-level `createStore` (R5), not React state, so any code path —
 * React components, the react-query error handler, the event stream — can
 * surface a notification without prop drilling. The bell button and panel
 * read it via `useStore`.
 */
import { createStore, useStore } from '@/lib/store'

import type { NotificationSeverity } from '@/domain/vocabulary'

interface NotificationAction {
  label: string
  run: () => void | Promise<void>
}

export interface AppNotification {
  id: string
  severity: NotificationSeverity
  title: string
  message?: string
  createdAt: number
  read: boolean
  /** Collapses repeated occurrences of the same condition into one entry. */
  dedupeKey?: string
  action?: NotificationAction
}

export type NotificationInput = Omit<
  AppNotification,
  'id' | 'createdAt' | 'read'
>

const MAX_NOTIFICATIONS = 100

const notificationStore = createStore<AppNotification[]>([])

/**
 * Add a notification (or refresh an existing one with the same `dedupeKey`,
 * bumping it to the top and marking it unread). Returns the notification id.
 */
export function pushNotification(input: NotificationInput): string {
  const notifications = notificationStore.get()
  if (input.dedupeKey) {
    const existing = notifications.find((n) => n.dedupeKey === input.dedupeKey)
    if (existing) {
      const refreshed: AppNotification = {
        ...existing,
        ...input,
        id: existing.id,
        createdAt: Date.now(),
        read: false,
      }
      notificationStore.set([
        refreshed,
        ...notifications.filter((n) => n.id !== existing.id),
      ])
      return existing.id
    }
  }
  const id =
    typeof crypto !== 'undefined' && 'randomUUID' in crypto
      ? crypto.randomUUID()
      : `n-${Date.now()}-${Math.random().toString(36).slice(2)}`
  notificationStore.set(
    [
      { ...input, id, createdAt: Date.now(), read: false },
      ...notifications,
    ].slice(0, MAX_NOTIFICATIONS),
  )
  return id
}

export function dismissNotification(id: string): void {
  notificationStore.set(notificationStore.get().filter((n) => n.id !== id))
}

export function clearNotifications(): void {
  if (notificationStore.get().length === 0) {
    return
  }
  notificationStore.set([])
}

export function markAllNotificationsRead(): void {
  const notifications = notificationStore.get()
  if (notifications.every((n) => n.read)) {
    return
  }
  notificationStore.set(notifications.map((n) => ({ ...n, read: true })))
}

/** Subscribe to the full, newest-first notification list. */
export function useNotifications(): AppNotification[] {
  return useStore(notificationStore)
}

/** Subscribe to the unread count (for the toolbar badge). */
export function useUnreadNotificationCount(): number {
  return useStore(notificationStore, (items) =>
    items.reduce((count, n) => (n.read ? count : count + 1), 0),
  )
}
