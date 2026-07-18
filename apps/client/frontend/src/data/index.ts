// The data layer the ported app runs on: one MailClient facade (transport,
// stream, generations), react-query as the mirror, and ONE invalidation
// policy — generation advance invalidates everything. Components import from
// here; they never see HTTP, SSE, or generations.

export { MailClientProvider, useMailClient, useOptionalMailClient } from './context'
export { queryClient } from './queries/queryClient'
export { familyKey, queryKeys } from '@/data/queries/queryKeys'
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
} from './queries/queries'
export { runCommand, useCommands, type MailCommands } from './transport/commands'
export { useDomainEvent, useStreamInvalidation } from './transport/stream'
export { useAccountLogoUrl, useBlobUrl } from './transport/blobs'
export { useConnectionStatus } from './transport/connection'
export type { MailSelection, MailViewSelection } from './models/selection'
