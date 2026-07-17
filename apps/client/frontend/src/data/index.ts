// The data layer the ported app runs on: one MailClient facade (transport,
// stream, generations), react-query as the mirror, and ONE invalidation
// policy — generation advance invalidates everything. Components import from
// here; they never see HTTP, SSE, or generations.

export { MailClientProvider, useMailClient, useOptionalMailClient } from './context'
export { queryClient } from './queryClient'
export { familyKey, queryKeys } from '@/data/queryKeys'
export {
  ensureAppSettings,
  fetchQuery,
  useAccountSettings,
  useAccounts,
  useAppSettings,
  useMailboxCounts,
  useMailList,
  useMessageDetail,
  useMessageRawSource,
  usePendingOperations,
  useRevLog,
  useSenderAddresses,
  useSmartMailboxes,
  useTags,
  useThread,
} from './queries'
export { runCommand, useCommands, type MailCommands } from './commands'
export { useDomainEvent, useStreamInvalidation } from './stream'
export { useAccountLogoUrl, useBlobUrl } from './blobs'
export { useConnectionStatus } from './connection'
export type { MailSelection, MailViewSelection } from './selection'
