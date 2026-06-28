import type { QueryClient } from '@tanstack/react-query'

import type { AccountOverview, Mailbox, SmartMailboxSummary } from '@/api/types'
import {
  ALL_MAIL_DEFAULT_KEY,
  KNOWN_MAILBOX_ROLES,
} from '@/domainVocabulary'
import { queryKeys } from '@/queryKeys'

import type { RuntimeMessagePageRequest, RuntimeMessagePageScope } from './types'

/**
 * The store's membership predicate for a mail-list view (mirrors the WASM
 * `ViewPredicate`). `inMailboxes` is set-intersection — a message matches if its
 * `mailboxIds` intersect the set. A concrete folder is a one-element set; a role
 * smart mailbox (e.g. "All Inboxes") is the role's mailbox in every account.
 */
export type MailListPredicate =
  | { inMailboxes: string[] }
  | 'all'
  | 'deferred'

/**
 * Resolution context for {@link resolveMailListPredicate}, read from the cached
 * navigation read models. Pure data so the resolver stays testable. Built once
 * per call via {@link buildMailListPredicateContext} from the same query cache
 * both adapters share — so the store predicate and the runtime
 * `clientSelfMaintained` flag never drift.
 */
export interface MailListPredicateContext {
  /** A smart mailbox's `defaultKey` (= its role for the built-in role mailboxes,
   *  `"all-mail"` for All Mail), or `null` for a user-defined smart mailbox. */
  smartMailboxDefaultKey: (smartMailboxId: string) => string | null | undefined
  /** Every mailbox id carrying `role`, across all accounts. */
  mailboxesForRole: (role: string) => string[]
}

/** The built-in role smart mailboxes whose membership is a single
 *  `mailboxRole == <role>` rule — evaluable from the firehose projection.
 *  Derived from {@link KNOWN_MAILBOX_ROLES} so the resolver + the role
 *  vocabulary cannot drift. */
const ROLE_DEFAULT_KEYS = new Set<string>(KNOWN_MAILBOX_ROLES)

/**
 * The store-side membership predicate for a mail-list view, or `'deferred'` when
 * the store cannot self-evaluate it (the runtime must re-serve per event).
 *
 * Evaluable iff the default `date` sort (rows keyed by `receivedAt`) over a
 * predicate the firehose projection can decide:
 * - a concrete source mailbox → `{ inMailboxes: [mailboxId] }`
 * - a built-in role smart mailbox (inbox/archive/…) → the role's mailbox in
 *   every account, `{ inMailboxes: [...] }`
 * - the All Mail smart mailbox (empty rule) → `'all'`
 *
 * Everything else — user smart mailboxes (opaque rules), global/search scopes,
 * null-mailbox, non-`date` sorts — is `'deferred'`. Unresolvable role lookups
 * (no mailbox carries the role yet) degrade to `'deferred'`: correct, just not
 * self-maintained.
 */
export function resolveMailListPredicate(
  scope: RuntimeMessagePageScope,
  sort: RuntimeMessagePageRequest['sort'],
  ctx: MailListPredicateContext,
): MailListPredicate {
  if (sort && sort !== 'date') return 'deferred'
  switch (scope.kind) {
    case 'source-mailbox':
      return scope.mailboxId != null
        ? { inMailboxes: [scope.mailboxId] }
        : 'deferred'
    case 'smart-mailbox': {
      const key = ctx.smartMailboxDefaultKey(scope.smartMailboxId)
      if (key == null) return 'deferred'
      if (key === ALL_MAIL_DEFAULT_KEY) return 'all'
      if (!ROLE_DEFAULT_KEYS.has(key)) return 'deferred'
      const ids = ctx.mailboxesForRole(key)
      return ids.length > 0 ? { inMailboxes: ids } : 'deferred'
    }
    case 'global':
      return 'deferred'
  }
}

/**
 * Whether the entity store self-maintains a mail-list view's membership from the
 * `message.updated` firehose. The runtime skips its per-event re-serve only for
 * self-maintained views; a deferred view MUST stay false so the runtime
 * re-serves it (else it stales until reload — the option-iii regression).
 *
 * Single source of truth: derived from {@link resolveMailListPredicate}, which
 * also produces the store's in-engine predicate — so the two cannot drift.
 */
export function isMailListSelfMaintained(
  scope: RuntimeMessagePageScope,
  sort: RuntimeMessagePageRequest['sort'],
  ctx: MailListPredicateContext,
): boolean {
  return resolveMailListPredicate(scope, sort, ctx) !== 'deferred'
}

/** Assemble a {@link MailListPredicateContext} from the cached navigation read
 *  models (`accounts`, per-account `mailboxes`, `smartMailboxes`). Empty maps
 *  when caches are unhydrated → role smart mailboxes degrade to `'deferred'`
 *  (the runtime re-serves; no regression) until the sidebar loads them. */
export function buildMailListPredicateContext(
  queryClient: QueryClient,
): MailListPredicateContext {
  const smartMailboxes =
    queryClient.getQueryData<SmartMailboxSummary[]>(queryKeys.smartMailboxes) ??
    []
  const defaultKeyById = new Map(
    smartMailboxes.map((mailbox) => [mailbox.id, mailbox.defaultKey]),
  )

  const accounts =
    queryClient.getQueryData<AccountOverview[]>(queryKeys.accounts) ?? []
  const idsByRole = new Map<string, string[]>()
  for (const account of accounts) {
    const mailboxes =
      queryClient.getQueryData<Mailbox[]>(queryKeys.mailboxes(account.id)) ?? []
    for (const mailbox of mailboxes) {
      if (!mailbox.role) continue
      const ids = idsByRole.get(mailbox.role) ?? []
      ids.push(mailbox.id)
      idsByRole.set(mailbox.role, ids)
    }
  }

  return {
    smartMailboxDefaultKey: (id) => defaultKeyById.get(id),
    mailboxesForRole: (role) => idsByRole.get(role) ?? [],
  }
}
