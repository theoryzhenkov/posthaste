/**
 * Account health presentation — the single client-side mapping from an
 * account's runtime status + classified error code into a user-facing
 * category, message, and recovery affordance (RFC-L2-client-resilience M45).
 *
 * The server already classifies raw provider/library errors into a stable
 * `lastSyncErrorCode` and a human `lastSyncError` message (never a raw string).
 * This layer re-classifies that code into a coarse category, adds
 * provider-aware phrasing, and decides which recovery action to offer, so the
 * sidebar account row, the settings accounts pane, and any toast all render the
 * same "what happened + what to do" from one place.
 *
 * @spec docs/eph/RFC-L2-client-resilience.md#m45
 */
import type { AccountOverview, AccountRuntime } from './api/types'

export type AccountErrorCategory =
  | 'network'
  | 'auth'
  | 'rateLimited'
  | 'config'
  | 'storage'
  | 'internal'

/** Severity tone used to pick colors/icons for the health indicator. */
export type AccountHealthSeverity = 'ok' | 'info' | 'warn' | 'error'

/** The recovery affordance to render for an unhealthy account. */
export type AccountRecoveryAction = 'retry' | 'reconnect' | 'edit' | null

export interface AccountHealth {
  status: AccountRuntime['status']
  severity: AccountHealthSeverity
  /** Short human label for the state (Connected / Syncing / Sign-in needed …). */
  label: string
  /** One-line human explanation, or null when the account is healthy. */
  message: string | null
  category: AccountErrorCategory | null
  /** Whether the supervisor is auto-retrying (so we say "retrying" not "act"). */
  autoRetrying: boolean
  /** The recovery action to offer, and its button label. */
  action: AccountRecoveryAction
  actionLabel: string | null
  /** True when the account needs attention (drives the global indicator). */
  isUnhealthy: boolean
}

interface CategoryPresentation {
  category: AccountErrorCategory
  autoRetrying: boolean
  action: AccountRecoveryAction
  actionLabel: string | null
  message: (provider: string) => string
}

const NETWORK: CategoryPresentation = {
  category: 'network',
  autoRetrying: true,
  action: 'retry',
  actionLabel: 'Retry now',
  message: (provider) =>
    `Couldn't reach ${provider} — check your connection. Retrying automatically.`,
}

const RATE_LIMITED: CategoryPresentation = {
  category: 'rateLimited',
  autoRetrying: true,
  action: 'retry',
  actionLabel: 'Retry now',
  message: (provider) =>
    `${provider} is throttling requests — retrying shortly.`,
}

const AUTH: CategoryPresentation = {
  category: 'auth',
  autoRetrying: false,
  action: 'reconnect',
  actionLabel: 'Reconnect',
  message: () => 'Sign-in expired — reconnect your account.',
}

const CONFIG: CategoryPresentation = {
  category: 'config',
  autoRetrying: false,
  action: 'edit',
  actionLabel: 'Edit settings',
  message: () =>
    'Server settings look wrong — check this account’s configuration.',
}

const STORAGE: CategoryPresentation = {
  category: 'storage',
  autoRetrying: false,
  action: 'retry',
  actionLabel: 'Retry now',
  message: () =>
    'A local database problem is affecting this account — a repair may be needed.',
}

const INTERNAL: CategoryPresentation = {
  category: 'internal',
  autoRetrying: true,
  action: 'retry',
  actionLabel: 'Retry now',
  message: () => 'Something went wrong syncing this account — retrying.',
}

/**
 * Map the stable `lastSyncErrorCode` (from the server classifier and the
 * supervisor status writers) to a category presentation. Unknown codes fall
 * back to the internal/degraded presentation rather than leaking through.
 */
function presentationForCode(code: string | null): CategoryPresentation {
  switch (code) {
    case 'network_error':
    case 'gateway_unavailable':
      return NETWORK
    case 'rate_limited':
      return RATE_LIMITED
    case 'auth_error':
    case 'secret_unavailable':
    case 'secret_unsupported':
      return AUTH
    case 'gateway_rejected':
    case 'state_mismatch':
    case 'cannot_calculate_changes':
    case 'config_validation':
    case 'config_parse':
    case 'config_io':
      return CONFIG
    case 'storage_corrupted':
    case 'storage_failure':
      return STORAGE
    default:
      // arm_timeout, runtime_fault, runtime_halted, push_terminal, internal,
      // not_found, conflict, or anything new: a degraded internal state.
      return INTERNAL
  }
}

const HEALTHY_LABEL: Partial<Record<AccountRuntime['status'], string>> = {
  ready: 'Connected',
  syncing: 'Syncing',
  disabled: 'Disabled',
}

function errorLabel(category: AccountErrorCategory): string {
  switch (category) {
    case 'auth':
      return 'Sign-in needed'
    case 'network':
      return 'Connection issue'
    case 'rateLimited':
      return 'Throttled'
    case 'config':
      return 'Settings issue'
    case 'storage':
      return 'Storage issue'
    case 'internal':
      return 'Degraded'
  }
}

/**
 * Classify an account's runtime state into its user-facing health presentation.
 *
 * `providerName` is folded into network/rate-limit phrasing (e.g. "Couldn't
 * reach Gmail"); pass the account display name when the provider is unknown.
 */
export function accountHealth(
  runtime: AccountRuntime,
  providerName: string,
): AccountHealth {
  const { status } = runtime

  // Healthy / benign states: no error, no action.
  if (status === 'ready' || status === 'syncing' || status === 'disabled') {
    return {
      status,
      severity: status === 'syncing' ? 'info' : 'ok',
      label: HEALTHY_LABEL[status] ?? status,
      message: null,
      category: null,
      autoRetrying: status === 'syncing',
      action: null,
      actionLabel: null,
      isUnhealthy: false,
    }
  }

  // Unhealthy: authError / offline / degraded. Derive the category from the
  // classified code; prefer our provider-aware phrasing, but never fall back to
  // a raw string — the server message is already humanized when present.
  const preset = presentationForCode(runtime.lastSyncErrorCode)
  // authError status always presents as Auth regardless of a stale code.
  const presentation = status === 'authError' ? AUTH : preset
  const message = presentation.message(providerName)
  const severity: AccountHealthSeverity =
    status === 'authError' || presentation.category === 'config'
      ? 'error'
      : 'warn'

  return {
    status,
    severity,
    label: errorLabel(presentation.category),
    message,
    category: presentation.category,
    autoRetrying: presentation.autoRetrying,
    action: presentation.action,
    actionLabel: presentation.actionLabel,
    isUnhealthy: true,
  }
}

/** Convenience over an `AccountOverview`, using its provider/display name. */
export function accountHealthFor(account: AccountOverview): AccountHealth {
  return accountHealth(account.runtime, providerDisplayName(account))
}

const PROVIDER_DISPLAY_NAMES: Record<string, string> = {
  gmail: 'Gmail',
  outlook: 'Outlook',
  icloud: 'iCloud',
}

/** Best-effort human provider name for phrasing (falls back to account name). */
export function providerDisplayName(account: AccountOverview): string {
  return PROVIDER_DISPLAY_NAMES[account.connection.providerKind] ?? account.name
}

/** The subset of enabled accounts currently needing attention (M45 global). */
export function unhealthyAccounts(
  accounts: AccountOverview[],
): AccountOverview[] {
  return accounts.filter(
    (account) => account.enabled && accountHealthFor(account).isUnhealthy,
  )
}
