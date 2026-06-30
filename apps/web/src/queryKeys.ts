/**
 * Canonical React Query key builders for app-level server state.
 *
 * @spec docs/L1-ui#data-fetching
 */
type MessageQuerySelection =
  | { kind: 'smart-mailbox'; id: string }
  | { kind: 'source-mailbox'; sourceId: string; mailboxId: string }
  | null

interface MessageQuerySort {
  columnId: string
  direction: string
}

export const queryKeys = {
  settings: ['settings'] as const,
  accounts: ['accounts'] as const,
  account: (accountId: string | null) => ['account', accountId] as const,
  identity: (sourceId: string) => ['identity', sourceId] as const,
  senderAddresses: ['sender-addresses'] as const,
  composeRecipientSuggestions: ['compose-recipient-suggestions'] as const,
  mailboxes: (accountId: string | null) => ['mailboxes', accountId] as const,
  pendingOperations: (accountId: string) =>
    ['pending-operations', accountId] as const,
  tags: ['tags'] as const,
  mailNavigationRead: ['read', 'mail-navigation'] as const,
  messagesRoot: ['messages'] as const,
  conversationsRoot: ['conversations'] as const,
  messageDetailsRoot: ['message'] as const,
  conversationDetailsRoot: ['conversation'] as const,
  conversationSummariesRoot: ['conversation-summary'] as const,
  messages: (
    selection: MessageQuerySelection,
    query?: string,
    sort?: MessageQuerySort,
  ) =>
    [
      'messages',
      selection,
      query?.trim() || null,
      sort ? { columnId: sort.columnId, direction: sort.direction } : null,
    ] as const,
  smartMailboxes: ['smart-mailboxes'] as const,
  smartMailboxRoot: ['smart-mailbox'] as const,
  smartMailbox: (smartMailboxId: string | null) =>
    ['smart-mailbox', smartMailboxId] as const,
}
