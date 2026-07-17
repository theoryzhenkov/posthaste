/**
 * Notification center panel: a command-palette-style floating panel listing all
 * surfaced errors and notices, newest first, each with an optional action.
 */
import { useState } from 'react'
import { AlertCircle, AlertTriangle, Info, Loader2, X } from 'lucide-react'

import { cn } from '@/lib/utils'
import {
  clearNotifications,
  dismissNotification,
  useNotifications,
  type AppNotification,
  type NotificationSeverity,
} from '@/notifications/store'

import { FloatingPanel } from './FloatingPanel'

const SEVERITY_ICON: Record<NotificationSeverity, typeof Info> = {
  error: AlertCircle,
  warning: AlertTriangle,
  info: Info,
}

const SEVERITY_CLASS: Record<NotificationSeverity, string> = {
  error: 'text-destructive',
  warning: 'text-amber-500',
  info: 'text-muted-foreground',
}

function relativeTime(timestamp: number): string {
  const seconds = Math.round((Date.now() - timestamp) / 1000)
  if (seconds < 60) return 'just now'
  const minutes = Math.round(seconds / 60)
  if (minutes < 60) return `${minutes}m ago`
  const hours = Math.round(minutes / 60)
  if (hours < 24) return `${hours}h ago`
  return `${Math.round(hours / 24)}d ago`
}

export function NotificationsPanel({ onClose }: { onClose: () => void }) {
  const notifications = useNotifications()

  return (
    <FloatingPanel
      panelLabel="notifications"
      storageKey="posthaste.notifications.panelOffset"
      sizePreset="command"
      className="flex flex-col"
      header={
        <div className="flex h-12 w-full items-center justify-between px-4">
          <p className="text-[13px] font-semibold text-foreground">
            Notifications
          </p>
          {notifications.length > 0 && (
            <button
              type="button"
              onClick={clearNotifications}
              className="ph-focus-ring rounded-[5px] px-2 py-1 text-[12px] text-muted-foreground transition-colors hover:bg-[var(--hover-bg)] hover:text-foreground"
            >
              Clear all
            </button>
          )}
        </div>
      }
      onClose={onClose}
    >
      <div className="min-h-0 flex-1 overflow-y-auto">
        {notifications.length === 0 ? (
          <div className="flex h-full min-h-[160px] items-center justify-center px-6 text-center text-[13px] text-muted-foreground">
            No notifications. Errors and notices will appear here.
          </div>
        ) : (
          <ul className="divide-y divide-border/60">
            {notifications.map((notification) => (
              <NotificationRow
                key={notification.id}
                notification={notification}
              />
            ))}
          </ul>
        )}
      </div>
    </FloatingPanel>
  )
}

function NotificationRow({ notification }: { notification: AppNotification }) {
  const Icon = SEVERITY_ICON[notification.severity]
  const [isRunning, setIsRunning] = useState(false)

  async function runAction() {
    if (!notification.action || isRunning) return
    setIsRunning(true)
    try {
      await notification.action.run()
    } finally {
      setIsRunning(false)
    }
  }

  return (
    <li className="group flex gap-3 px-4 py-3">
      <Icon
        size={16}
        strokeWidth={1.8}
        className={cn('mt-0.5 shrink-0', SEVERITY_CLASS[notification.severity])}
      />
      <div className="min-w-0 flex-1">
        <div className="flex items-start justify-between gap-2">
          <p className="text-[13px] font-medium text-foreground">
            {notification.title}
          </p>
          <span className="shrink-0 font-mono text-[10px] text-muted-foreground">
            {relativeTime(notification.createdAt)}
          </span>
        </div>
        {notification.message && (
          <p className="mt-0.5 text-[12px] leading-5 text-muted-foreground">
            {notification.message}
          </p>
        )}
        {notification.action && (
          <button
            type="button"
            disabled={isRunning}
            onClick={() => void runAction()}
            className="ph-focus-ring mt-2 inline-flex items-center gap-1.5 rounded-[5px] border border-border bg-background px-2.5 py-1 text-[12px] font-medium text-foreground transition-colors hover:bg-[var(--hover-bg)] disabled:opacity-50"
          >
            {isRunning && <Loader2 size={12} className="animate-spin" />}
            {notification.action.label}
          </button>
        )}
      </div>
      <button
        type="button"
        aria-label="Dismiss notification"
        onClick={() => dismissNotification(notification.id)}
        className="ph-focus-ring h-5 shrink-0 self-start rounded-[4px] text-muted-foreground opacity-0 transition-opacity hover:text-foreground group-hover:opacity-100"
      >
        <X size={13} strokeWidth={1.8} />
      </button>
    </li>
  )
}
