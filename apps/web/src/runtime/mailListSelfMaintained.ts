import type { RuntimeMessagePageRequest, RuntimeMessagePageScope } from './types'

/**
 * Whether a mail-list view's membership is client-self-maintained by the entity
 * store — i.e. the store can evaluate the predicate itself from the
 * `message.updated` firehose (an *evaluable* predicate): the default `date` sort
 * (which keys rows by `receivedAt`) over a concrete source mailbox.
 *
 * Smart-mailbox / global / null-mailbox / non-`date` views are `Deferred` — the
 * store cannot self-maintain them, so the runtime must re-serve them per
 * affecting event. Option iii (`single-source-view-membership`) skips the
 * per-event re-serve only for self-maintained views; skipping it for a Deferred
 * view stales it until reload (the option-iii regression).
 *
 * Single source of truth: both the store's predicate derivation
 * (`predicateForScope`) and the runtime's `clientSelfMaintained` descriptor flag
 * call this. The runtime reads the flag and never re-derives — so there is no
 * TS↔Rust drift.
 */
export function isMailListSelfMaintained(
  scope: RuntimeMessagePageScope,
  sort: RuntimeMessagePageRequest['sort'],
): scope is { kind: 'source-mailbox'; sourceId: string; mailboxId: string } {
  if (sort && sort !== 'date') return false
  return scope.kind === 'source-mailbox' && scope.mailboxId != null
}
