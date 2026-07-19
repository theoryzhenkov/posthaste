// The data layer the ported app runs on: one MailClient facade (transport,
// stream, generations), react-query as the mirror, and ONE invalidation
// policy — generation advance invalidates everything. Components import from
// here; they never see HTTP, SSE, or generations.

export { MailClientProvider, useMailClient } from './context'
export { queryClient } from './queries/queryClient'
export { queryKeys } from '@/data/queries/queryKeys'
export {
  fetchQuery,
  useAccountSettings,
  useAccounts,
  useAppSettings,
  useMailboxCounts,
  usePendingOperations,
  useRevLog,
  useSenderAddresses,
  useSmartMailboxes,
  useTags,
} from './queries/queries'
export { useCommands } from './transport/commands'
export { useDomainEvent, useStreamInvalidation } from './transport/stream'
export { useConnectionStatus } from './transport/connection'
export type { MailSelection } from './models/selection'
