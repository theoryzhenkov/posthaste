/**
 * Canonical React Query key builders for app-level server state.
 *
 * @spec docs/L1-ui#data-fetching
 */
type MessageQuerySelection =
  | { kind: 'smart-mailbox'; id: string }
  | { kind: 'source-mailbox'; sourceId: string; mailboxId: string }
  | null

export const queryKeys = {
  settings: ['settings'] as const,
  accounts: ['accounts'] as const,
  account: (accountId: string | null) => ['account', accountId] as const,
  sidebar: ['sidebar'] as const,
  messagesRoot: ['messages'] as const,
  messages: (selection: MessageQuerySelection) =>
    ['messages', selection] as const,
  smartMailboxes: ['smart-mailboxes'] as const,
  smartMailbox: (smartMailboxId: string | null) =>
    ['smart-mailbox', smartMailboxId] as const,
}
