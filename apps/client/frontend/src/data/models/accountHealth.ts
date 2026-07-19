/**
 * Account health presentation — the single client-side mapping from an
 * account's live status into a user-facing category, message, and recovery
 * affordance. The sidebar account row, the settings accounts pane, and any
 * toast all render the same "what happened + what to do" from one place.
 *
 * The `accounts` query serves each account's supervisor-owned `status` plus a
 * humanized `lastSyncError` message (never a raw provider string). The status
 * carries the classification: `authError` and `offline` are their own states,
 * and everything else unhealthy presents as degraded-with-retry, preferring
 * the server's message when one is present.
 */
import { MailApiError } from '@/data/transport/client'
import type { AccountRow, AccountStatus } from '@/gen'
import type { AccountHealthSeverity } from '@/domain/vocabulary'

type AccountErrorCategory =
  | 'network'
  | 'auth'
  | 'rateLimited'
  | 'config'
  | 'storage'
  | 'internal'

/** The recovery affordance to render for an unhealthy account. */
type AccountRecoveryAction = 'retry' | 'reconnect' | 'edit' | null

export interface AccountHealth {
  status: AccountStatus
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

const HEALTHY_LABEL: Partial<Record<AccountStatus, string>> = {
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
 * Classify an account's live state into its user-facing health presentation.
 *
 * `providerName` is folded into network phrasing (e.g. "Couldn't reach
 * Gmail"); pass the account display name when the provider is unknown.
 */
export function accountHealth(
  row: Pick<AccountRow, 'status' | 'lastSyncError'>,
  providerName: string,
): AccountHealth {
  const { status } = row

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

  if (status === 'authError') {
    return {
      status,
      severity: 'error',
      label: errorLabel('auth'),
      message: 'Sign-in expired — reconnect your account.',
      category: 'auth',
      autoRetrying: false,
      action: 'reconnect',
      actionLabel: 'Reconnect',
      isUnhealthy: true,
    }
  }

  if (status === 'offline') {
    return {
      status,
      severity: 'warn',
      label: errorLabel('network'),
      message: `Couldn't reach ${providerName} — check your connection. Retrying automatically.`,
      category: 'network',
      autoRetrying: true,
      action: 'retry',
      actionLabel: 'Retry now',
      isUnhealthy: true,
    }
  }

  // degraded: prefer the server's humanized message when it sent one.
  return {
    status,
    severity: 'warn',
    label: errorLabel('internal'),
    message:
      row.lastSyncError ??
      'Something went wrong syncing this account — retrying.',
    category: 'internal',
    autoRetrying: true,
    action: 'retry',
    actionLabel: 'Retry now',
    isUnhealthy: true,
  }
}

/** Convenience over an `AccountRow`, using its display name for phrasing. */
export function accountHealthFor(account: AccountRow): AccountHealth {
  return accountHealth(account, account.name)
}

/**
 * Classification of an add/edit/verify-account failure into the same coarse
 * {@link AccountErrorCategory} space used by the live health surface, so the
 * onboarding form speaks the recovery-UX vocabulary instead of a raw string.
 *
 * Setup failures arrive as a typed {@link MailApiError}; its `kind` maps onto
 * the coarse categories (validation → config, unauthorized → auth, …).
 */
export interface AccountSetupError {
  category: AccountErrorCategory
  message: string
}

function setupCategoryForKind(
  kind: MailApiError['kind'] | null,
): AccountErrorCategory {
  switch (kind) {
    case 'malformedRequest':
    case 'unknownId':
      return 'config'
    case 'unauthorized':
    case 'capabilityDenied':
      return 'auth'
    case 'unavailable':
      return 'network'
    case 'conflict':
    case 'internal':
    default:
      return 'internal'
  }
}

const SETUP_MESSAGES: Record<AccountErrorCategory, string> = {
  auth: 'Sign-in was rejected — check the username and password.',
  network:
    "Couldn't reach the mail server — check the host, port, and your connection.",
  rateLimited: 'The mail server is throttling requests — try again shortly.',
  config:
    'The server settings look wrong — check the host, port, and security.',
  storage: 'A local database problem blocked saving this account.',
  internal: 'Something went wrong setting up this account — try again.',
}

/**
 * Turn a create/verify failure into a classified, human setup message. Never
 * surfaces a raw provider/library string except the server's own humanized
 * validation message. `appPasswordHint`, when provided, is appended to auth
 * failures (e.g. the Fastmail/iCloud/Gmail app-password reminder).
 */
export function classifyAccountSetupError(
  error: unknown,
  appPasswordHint?: string | null,
): AccountSetupError {
  const kind = error instanceof MailApiError ? error.kind : null
  const category = setupCategoryForKind(kind)
  let message = SETUP_MESSAGES[category]

  // A malformed-request rejection carries the server's own field-level
  // validation message — more specific than the category default.
  if (
    error instanceof MailApiError &&
    kind === 'malformedRequest' &&
    error.message
  ) {
    message = error.message
  }

  if (category === 'auth' && appPasswordHint) {
    message = `${message} ${appPasswordHint}`
  }
  return { category, message }
}

/** The subset of enabled accounts currently needing attention. */
export function unhealthyAccounts(accounts: AccountRow[]): AccountRow[] {
  return accounts.filter(
    (account) => account.enabled && accountHealthFor(account).isUnhealthy,
  )
}
