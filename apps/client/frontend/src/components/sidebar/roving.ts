/**
 * Pure helpers for the sidebar's `j`/`k` roving cursor.
 *
 */
import type { SidebarSelection } from '../Sidebar'

/** Stable key for a navigable sidebar row (smart mailbox, tag, or folder). */
export type SidebarNavKey = string

export function smartNavKey(id: string): SidebarNavKey {
  return `smart:${id}`
}

export function tagNavKey(name: string): SidebarNavKey {
  return `tag:${name}`
}

export function sourceNavKey(
  sourceId: string,
  mailboxId: string,
): SidebarNavKey {
  return `src:${sourceId}:${mailboxId}`
}

/** The nav key of the row backing the current view, if any (tags excluded — a
 *  tag selection drives a search, not a sidebar view). */
export function sidebarSelectionKey(
  view: SidebarSelection | null,
): SidebarNavKey | null {
  if (!view) return null
  if (view.kind === 'smart-mailbox') return smartNavKey(view.id)
  return sourceNavKey(view.sourceId, view.mailboxId)
}

/**
 * Move the roving cursor by one row, clamped at the ends (no wrap, matching the
 * message list). A null `currentKey` starts from the first/last row.
 */
export function moveRovingKey(
  keys: readonly SidebarNavKey[],
  currentKey: SidebarNavKey | null,
  direction: 1 | -1,
): SidebarNavKey | null {
  if (keys.length === 0) return null
  const index = currentKey ? keys.indexOf(currentKey) : -1
  if (index === -1) return direction === 1 ? keys[0] : keys[keys.length - 1]
  const next = Math.min(Math.max(index + direction, 0), keys.length - 1)
  return keys[next]
}
