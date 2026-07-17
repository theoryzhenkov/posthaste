import type { ProviderState, SearchProvider } from './types'

// Remote (backend) provider dispatch is debounced by this much while the user
// is still typing, so each keystroke does not issue a backend request.
export const REMOTE_DEBOUNCE_MS = 180
// Safety net only: Best matches normally freezes when every provider settles
// (see the settlement effect). This caps the wait if a request never resolves.
export const SETTLEMENT_HARD_CAP_MS = 2000

const PROVIDER_LIMITS: Record<string, number> = {
  commands: 20,
  'query-completions': 12,
  mailboxes: 16,
  tags: 12,
  messages: 12,
}

export function emptyProviderState(): ProviderState {
  return {
    status: 'idle',
    candidates: [],
    nextCursor: null,
  }
}

export function initialProviderStates(
  providers: SearchProvider[],
): Map<string, ProviderState> {
  return new Map(
    providers.map((provider) => [
      provider.id,
      {
        ...emptyProviderState(),
        status: 'loading' as const,
      },
    ]),
  )
}

export function providerLimit(providerId: string): number {
  return PROVIDER_LIMITS[providerId] ?? 8
}

export function cloneStatesWith(
  states: Map<string, ProviderState>,
  providerId: string,
  update: (state: ProviderState) => ProviderState,
): Map<string, ProviderState> {
  const next = new Map(states)
  next.set(providerId, update(next.get(providerId) ?? emptyProviderState()))
  return next
}

export function allProvidersSettled(
  states: Map<string, ProviderState>,
): boolean {
  return [...states.values()].every((state) => state.status !== 'loading')
}

export function queryShape(query: string) {
  return {
    queryLength: query.length,
    queryTokenCount: query.trim() ? query.trim().split(/\s+/).length : 0,
  }
}
