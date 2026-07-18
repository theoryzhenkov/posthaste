// ONE flat, family-keyed scheme: `[family, canonicalArgs]`. Keys exist only
// to give equal queries one cache entry — never to target invalidation. The
// single invalidation policy (stream.ts) invalidates every active query on a
// generation advance, so there are no granular roots, no per-entity keys, and
// no key hierarchies to keep in sync.

import { canonicalQueryKey } from '@/data/transport/client'
import type {
  AccountId,
  MailListQuery,
  MessageDetailQuery,
  MessageRawSourceQuery,
  Query,
  RevLogQuery,
  ThreadQuery,
} from '@/gen'

/** Canonical key for any query body: `[family, canonicalized args]`. */
export function familyKey(query: Query): readonly [string, string] {
  const family = Object.keys(query)[0]!
  return [family, canonicalQueryKey(query)] as const
}

export const queryKeys = {
  mailList: (q: MailListQuery) => familyKey({ mailList: q }),
  thread: (q: ThreadQuery) => familyKey({ thread: q }),
  messageDetail: (q: MessageDetailQuery) => familyKey({ messageDetail: q }),
  messageRawSource: (q: MessageRawSourceQuery) => familyKey({ messageRawSource: q }),
  mailboxCounts: (accountId?: AccountId) => familyKey({ mailboxCounts: { accountId } }),
  accounts: familyKey({ accounts: {} }),
  accountSettings: (accountId: AccountId) => familyKey({ accountSettings: { accountId } }),
  pendingOperations: (accountId?: AccountId) =>
    familyKey({ pendingOperations: { accountId } }),
  appSettings: familyKey({ appSettings: {} }),
  smartMailboxes: familyKey({ smartMailboxes: {} }),
  tags: (accountId?: AccountId) => familyKey({ tags: { accountId } }),
  revLog: (q: RevLogQuery) => familyKey({ revLog: q }),
  senderAddresses: (accountId?: AccountId) => familyKey({ senderAddresses: { accountId } }),
}
