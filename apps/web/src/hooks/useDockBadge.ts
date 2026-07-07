/**
 * macOS Dock (and, where supported, Windows/Linux taskbar) unread badge — the
 * count selection + Tauri sink. The `DockBadge` component (DockBadge.tsx) mounts
 * this at the app root.
 *
 * The badge shows the total INBOX unread across all enabled accounts — the
 * standard "unread mail" counter (Apple Mail behaviour): only mailboxes with the
 * `inbox` role are summed, so spam/archive/sent never inflate the count. The
 * count is live: it reads the react-query mailbox rows' `unreadEmails`
 * (RFC-L2-count-unification), kept fresh by count invalidation on events plus
 * the optimistic overlay's setQueryData for the user's own mutations.
 *
 * Tauri-only. Outside the desktop webview (browser build, dev, tests) it is a
 * pure no-op — the badge push is guarded on `isTauriRuntime()` and every call is
 * best-effort (try/catch, debug-logged) so a badge failure never breaks the app.
 *
 * Badge API (Tauri v2): `getCurrentWindow().setBadgeCount(count?)` from
 * `@tauri-apps/api/window`; `undefined`/`0` clears the badge. It is app-wide, not
 * per-window, so only the main mail window drives it.
 */
import { getCurrentWindow } from '@tauri-apps/api/window'
import { useEffect, useRef } from 'react'

import type { Mailbox } from '@/api/types'
import { isTauriRuntime } from '@/desktop'
import { LOG_EVENTS, uiLogger } from '@/logger'

const INBOX_ROLE = 'inbox'

/**
 * Sum inbox-role unread for one account from the react-query mailbox rows.
 * Non-inbox roles (junk/archive/sent/…) are excluded.
 */
export function accountInboxUnread(mailboxes: readonly Mailbox[]): number {
  let total = 0
  for (const mailbox of mailboxes) {
    if (mailbox.role !== INBOX_ROLE) {
      continue
    }
    total += mailbox.unreadEmails
  }
  return total
}

async function applyDockBadge(count: number): Promise<void> {
  try {
    // `undefined` clears the badge; a positive count sets it. macOS shows the
    // Dock tile counter; unsupported platforms no-op inside Tauri.
    await getCurrentWindow().setBadgeCount(count > 0 ? count : undefined)
  } catch (error) {
    uiLogger.debug(
      { event: LOG_EVENTS.dockBadgeUpdateFailed, error: String(error) },
      'dock badge update failed',
    )
  }
}

/**
 * Push `unreadCount` to the app icon badge, but only in Tauri and only when it
 * CHANGES (redundant pushes are suppressed). A no-op outside the desktop webview.
 */
export function useDockBadge(unreadCount: number): void {
  const previous = useRef<number | null>(null)
  useEffect(() => {
    if (previous.current === unreadCount) {
      return
    }
    previous.current = unreadCount
    if (!isTauriRuntime()) {
      return
    }
    void applyDockBadge(unreadCount)
  }, [unreadCount])
}
