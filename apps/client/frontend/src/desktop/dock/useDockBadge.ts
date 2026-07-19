/**
 * macOS Dock (and, where supported, Windows/Linux taskbar) unread badge — the
 * count selection + Tauri sink. The `DockBadge` component (DockBadge.tsx) mounts
 * this at the app root.
 *
 * The badge shows the total INBOX unread across all enabled accounts — the
 * standard "unread mail" counter (Apple Mail behaviour): only mailboxes with the
 * `inbox` role are summed, so spam/archive/sent never inflate the count. The
 * count is live: it reads the `mailboxCounts` rows' `unreadEmails`, kept fresh
 * by the global generation-advance invalidation.
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

import type { MailboxCountsRow } from '@/gen'
import { isTauriRuntime } from '@/lib/platform/runtime'
import { MAILBOX_ROLES } from '@/domain/vocabulary'
import { LOG_EVENTS, uiLogger } from '@/lib/log/logger'

const INBOX_ROLE = MAILBOX_ROLES.Inbox

/**
 * Sum inbox-role unread across the given accounts from `mailboxCounts` rows.
 * Non-inbox roles (junk/archive/sent/…) are excluded.
 */
export function inboxUnreadTotal(
  rows: readonly MailboxCountsRow[],
  accountIds: ReadonlySet<string>,
): number {
  let total = 0
  for (const row of rows) {
    if (row.mailbox.role !== INBOX_ROLE || !accountIds.has(row.accountId)) {
      continue
    }
    total += row.mailbox.unreadEmails
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
