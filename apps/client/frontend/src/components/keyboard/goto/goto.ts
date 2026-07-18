/**
 * Pure logic for the `g`-prefixed "go to mailbox" commands.
 *
 *  - `gi` / `ga` / `gt` — inbox / archive / trash, resolved against context: in
 *    a source mailbox they jump to that account's role folder; anywhere else
 *    (or when the account lacks that folder) they fall back to the matching
 *    smart mailbox.
 *  - `gq{i,a,t,d,s,j}` — always the smart mailbox for the given role, ignoring
 *    context.
 *
 */
import type { MailViewSelection } from '@/data/models/selection'
import {
  ASSIGNABLE_MAILBOX_ROLES,
  MAILBOX_ROLES,
} from '@/domain/vocabulary'

/** Mailbox roles reachable via a goto command: the user-assignable provider
 *  roles (everything but the system-managed snooze). */
export type GotoRole = (typeof ASSIGNABLE_MAILBOX_ROLES)[number]

/** Pending state of the goto prefix machine. */
export type GotoPrefix = 'g' | 'gq' | null

/** Roles addressable by the top-level `g` prefix (context-aware). */
const G_ROLE: Record<string, GotoRole> = {
  i: MAILBOX_ROLES.Inbox,
  a: MAILBOX_ROLES.Archive,
  t: MAILBOX_ROLES.Trash,
}

/** Roles addressable by the `gq` prefix (always a smart mailbox). */
const GQ_ROLE: Record<string, GotoRole> = {
  i: MAILBOX_ROLES.Inbox,
  a: MAILBOX_ROLES.Archive,
  t: MAILBOX_ROLES.Trash,
  d: MAILBOX_ROLES.Drafts,
  s: MAILBOX_ROLES.Sent,
  j: MAILBOX_ROLES.Junk,
}

export type GotoPrefixStep =
  | { type: 'goto'; role: GotoRole; forceSmart: boolean }
  | { type: 'goto-conversation' }
  | { type: 'await-q' }
  | { type: 'cancel' }

/**
 * Advance the prefix machine by one key. `cancel` means the key wasn't part of
 * a goto sequence and should be handled normally.
 */
export function stepGotoPrefix(
  prefix: GotoPrefix,
  key: string,
): GotoPrefixStep {
  const lower = key.toLowerCase()
  if (prefix === 'g') {
    if (lower === 'q') return { type: 'await-q' }
    // `gc` — filter to the current message's conversation ("goto conversation").
    if (lower === 'c') return { type: 'goto-conversation' }
    const role = G_ROLE[lower]
    return role ? { type: 'goto', role, forceSmart: false } : { type: 'cancel' }
  }
  if (prefix === 'gq') {
    const role = GQ_ROLE[lower]
    return role ? { type: 'goto', role, forceSmart: true } : { type: 'cancel' }
  }
  return { type: 'cancel' }
}

export type GotoTarget =
  | { kind: 'smart-mailbox'; id: string; name: string }
  | {
      kind: 'source-mailbox'
      sourceId: string
      mailboxId: string
      mailboxName: string
    }

interface RoleMailbox {
  id: string
  name: string
  role: string | null
}

interface RoleSmartMailbox {
  id: string
  name: string
  role: string | null
  kind: string
}

/** Pick the smart mailbox for a role, preferring the built-in default. */
function pickSmartMailboxByRole(
  smartMailboxes: readonly RoleSmartMailbox[],
  role: GotoRole,
): RoleSmartMailbox | null {
  const matches = smartMailboxes.filter((mailbox) => mailbox.role === role)
  if (matches.length === 0) return null
  return matches.reduce((best, candidate) => {
    const bestDefault = best.kind === 'default' ? 0 : 1
    const candDefault = candidate.kind === 'default' ? 0 : 1
    if (candDefault !== bestDefault)
      return candDefault < bestDefault ? candidate : best
    // Same role + default-ness: keep the earlier one (smartMailboxes arrive in
    // display order, which the sidebar reorder persists).
    return best
  })
}

/**
 * Resolve a goto request to a concrete navigation target, or `null` when no
 * mailbox carries the role (caller decides how to surface the dead end).
 */
export function resolveGotoTarget(input: {
  effectiveView: MailViewSelection
  role: GotoRole
  forceSmart: boolean
  sourceMailboxes: readonly RoleMailbox[]
  smartMailboxes: readonly RoleSmartMailbox[]
}): GotoTarget | null {
  const { effectiveView, role, forceSmart, sourceMailboxes, smartMailboxes } =
    input

  if (!forceSmart && effectiveView?.kind === 'source-mailbox') {
    const mailbox = sourceMailboxes.find((candidate) => candidate.role === role)
    if (mailbox) {
      return {
        kind: 'source-mailbox',
        sourceId: effectiveView.sourceId,
        mailboxId: mailbox.id,
        mailboxName: mailbox.name,
      }
    }
    // No such folder in this account — fall through to the smart mailbox.
  }

  const smart = pickSmartMailboxByRole(smartMailboxes, role)
  return smart
    ? { kind: 'smart-mailbox', id: smart.id, name: smart.name }
    : null
}
