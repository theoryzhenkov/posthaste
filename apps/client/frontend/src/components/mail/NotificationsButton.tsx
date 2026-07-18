/**
 * Toolbar bell button + unread badge that opens the notification center.
 *
 * Self-contained: reads the module-level notification store directly, so it can
 * drop into the toolbar without prop drilling through the mail client.
 */
import { useState } from 'react'
import { Bell } from 'lucide-react'

import { cn } from '@/lib/cn'
import {
  markAllNotificationsRead,
  useUnreadNotificationCount,
} from '@/data/notifications/store'

import { NotificationsPanel } from './NotificationsPanel'

export function NotificationsButton() {
  const [isOpen, setIsOpen] = useState(false)
  const unread = useUnreadNotificationCount()

  function open() {
    setIsOpen(true)
    markAllNotificationsRead()
  }

  return (
    <>
      <button
        type="button"
        title="Notifications"
        aria-label={
          unread > 0 ? `Notifications (${unread} unread)` : 'Notifications'
        }
        onClick={() => (isOpen ? setIsOpen(false) : open())}
        className={cn(
          'ph-focus-ring relative flex size-7 items-center justify-center rounded-[6px] text-chrome-foreground/60 transition-colors hover:bg-[var(--hover-bg)] hover:text-chrome-foreground',
          isOpen && 'bg-[var(--hover-bg)] text-chrome-foreground',
        )}
      >
        <Bell size={14} strokeWidth={1.6} />
        {unread > 0 && (
          <span className="absolute -right-0.5 -top-0.5 flex h-3.5 min-w-3.5 items-center justify-center rounded-full bg-brand-coral px-1 text-[9px] font-semibold leading-none text-white">
            {unread > 9 ? '9+' : unread}
          </span>
        )}
      </button>
      {isOpen && <NotificationsPanel onClose={() => setIsOpen(false)} />}
    </>
  )
}
