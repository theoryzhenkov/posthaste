/**
 * App-wide notification center.
 *
 * A module-level store (not React state) so any code path — React components,
 * the react-query error handler, the event stream — can surface a notification
 * without prop drilling. The bell button and panel read it via
 * `useSyncExternalStore`.
 */
import { useSyncExternalStore } from 'react'

import type { NotificationSeverity } from '@/domain/vocabulary'

export interface NotificationAction {
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

let notifications: AppNotification[] = []
const listeners = new Set<() => void>()

function emit() {
  for (const listener of listeners) {
    listener()
  }
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener)
  return () => listeners.delete(listener)
}

function getSnapshot(): AppNotification[] {
  return notifications
}

/** Current notifications, newest-first. For non-React readers and tests. */
export function getNotificationsSnapshot(): readonly AppNotification[] {
  return notifications
}

/**
 * Add a notification (or refresh an existing one with the same `dedupeKey`,
 * bumping it to the top and marking it unread). Returns the notification id.
 */
export function pushNotification(input: NotificationInput): string {
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
      notifications = [
        refreshed,
        ...notifications.filter((n) => n.id !== existing.id),
      ]
      emit()
      return existing.id
    }
  }
  const id =
    typeof crypto !== 'undefined' && 'randomUUID' in crypto
      ? crypto.randomUUID()
      : `n-${Date.now()}-${Math.random().toString(36).slice(2)}`
  notifications = [
    { ...input, id, createdAt: Date.now(), read: false },
    ...notifications,
  ].slice(0, MAX_NOTIFICATIONS)
  emit()
  return id
}

export function dismissNotification(id: string): void {
  notifications = notifications.filter((n) => n.id !== id)
  emit()
}

export function clearNotifications(): void {
  if (notifications.length === 0) {
    return
  }
  notifications = []
  emit()
}

export function markAllNotificationsRead(): void {
  if (notifications.every((n) => n.read)) {
    return
  }
  notifications = notifications.map((n) => ({ ...n, read: true }))
  emit()
}

/** Subscribe to the full, newest-first notification list. */
export function useNotifications(): AppNotification[] {
  return useSyncExternalStore(subscribe, getSnapshot, getSnapshot)
}

/** Subscribe to the unread count (for the toolbar badge). */
export function useUnreadNotificationCount(): number {
  const items = useNotifications()
  return items.reduce((count, n) => (n.read ? count : count + 1), 0)
}
