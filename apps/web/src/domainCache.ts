/**
 * Centralized React Query cache updates for domain events and mutations.
 *
 * @spec docs/L1-ui#data-fetching
 * @spec docs/L1-api#sse-event-stream
 */
export {
  applyAccountMutationResult,
  mergeAccountOverview,
  removeAccountOverview,
} from './domain-cache/accounts'
export { applyDomainEvent } from './domain-cache/handlers'
export { isDomainEventShape } from './domain-cache/payload'
export {
  invalidateAccountReadModels,
  invalidateComposeSendReadModels,
  invalidateSmartMailboxMutationReadModels,
  invalidateSyncStartedReadModels,
} from './domain-cache/invalidations'
