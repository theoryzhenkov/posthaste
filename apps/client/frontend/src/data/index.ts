// The data layer the ported app runs on: one MailClient facade (transport,
// stream, generations), react-query as the mirror, and ONE invalidation
// policy — generation advance invalidates everything. Components import from
// here; they never see HTTP, SSE, or generations.
//
// The mirror is reached through `MirrorProvider` and `useQueryClient()`, never
// as a module-level client: the provider is what subscribes this window's
// cache to the stream, so there is no way to hold a mirror that nothing keeps
// live (queries/mirror.tsx).

export { MailClientProvider, useMailClient } from './context'
export { MirrorProvider } from './queries/mirror'
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
export { useDomainEvent } from './transport/stream'
export { useConnectionStatus } from './transport/connection'
export type { MailSelection } from './models/selection'
