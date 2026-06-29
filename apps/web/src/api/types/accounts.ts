import type {
  AccountDriver,
  MailEndpointSettings,
  ProviderAuthKind,
  ProviderHint,
  ProviderKind,
  SecretStatus,
} from './providers'

interface AccountConnectionOverviewBase {
  provider: ProviderHint
  providerKind: ProviderKind
  auth: ProviderAuthKind
  username: string | null
  imap: MailEndpointSettings | null
  smtp: MailEndpointSettings | null
  secret: SecretStatus
}

export interface ManualCredentialsAccountConnectionOverview extends AccountConnectionOverviewBase {
  kind: 'manualCredentials'
  auth: 'password' | 'appPassword'
  baseUrl: string | null
}

export interface ManagedOAuthAccountConnectionOverview extends AccountConnectionOverviewBase {
  kind: 'managedOAuth'
  auth: 'oauth2'
}

export type AccountConnectionOverview =
  | ManualCredentialsAccountConnectionOverview
  | ManagedOAuthAccountConnectionOverview

/**
 * Summary of a configured account, including transport and sync status.
 * @spec docs/L1-api#account-crud-lifecycle
 */
export interface AccountOverview {
  id: string
  name: string
  fullName: string | null
  signature: string | null
  emailPatterns: string[]
  driver: AccountDriver
  enabled: boolean
  appearance: AccountAppearance
  connection: AccountConnectionOverview
  createdAt: string
  updatedAt: string
  isDefault: boolean
  /**
   * Volatile runtime state, owned by the account supervisor. Nested so the UI
   * updates config and runtime through independent paths (config mutations vs
   * status events) without the two racing inside one flat object.
   */
  runtime: AccountRuntime
}

export type AccountStatus =
  | 'ready'
  | 'syncing'
  | 'degraded'
  | 'authError'
  | 'offline'
  | 'disabled'

export type PushStatus =
  | 'connected'
  | 'reconnecting'
  | 'unsupported'
  | 'disabled'

export interface AccountRuntime {
  status: AccountStatus
  push: PushStatus
  lastSyncAt: string | null
  lastSyncError: string | null
  lastSyncErrorCode: string | null
  syncProgress: SyncProgress | null
}

export interface SyncProgress {
  syncId: string
  trigger: 'startup' | 'poll' | 'push' | 'manual'
  startedAt: string
  stage:
    | 'connecting'
    | 'discovering'
    | 'planning'
    | 'fetching'
    | 'storing'
    | 'waiting'
  detail: string
  mailboxName: string | null
  mailboxIndex: number | null
  mailboxCount: number | null
  messageCount: number | null
  totalCount: number | null
}

/** @spec docs/L1-api#compose */
export interface CachedSenderAddress {
  sourceId: string
  name: string | null
  email: string
  lastUsedAt: string
}

/** @spec docs/L1-api#account-crud-lifecycle */
export type AccountAppearance =
  | {
      kind: 'initials'
      initials: string
      colorHue: number
    }
  | {
      kind: 'image'
      imageId: string
      initials: string
      colorHue: number
    }

/** @spec docs/L1-api#account-crud-lifecycle */
export interface AccountTransportInput {
  provider?: ProviderHint
  auth?: ProviderAuthKind
  baseUrl: string
  username: string
  imap?: MailEndpointSettings
  smtp?: MailEndpointSettings
}

/**
 * Tri-state secret write mode: keep existing, replace with new password, or clear.
 * @spec docs/L1-api#secret-management
 */
export interface SecretInstructionInput {
  mode: 'keep' | 'replace' | 'clear'
  password?: string
}

/** @spec docs/L1-api#account-crud-lifecycle */
export interface CreateAccountInput {
  id?: string
  name: string
  fullName?: string | null
  signature?: string | null
  emailPatterns: string[]
  driver?: AccountDriver
  enabled?: boolean
  appearance?: AccountAppearance
  transport: AccountTransportInput
  secret: SecretInstructionInput
}

/**
 * Sparse-merge update payload -- omitted fields are preserved.
 * @spec docs/L1-api#account-crud-lifecycle
 */
export interface UpdateAccountInput {
  name?: string
  fullName?: string | null
  signature?: string | null
  emailPatterns?: string[]
  driver?: AccountDriver
  enabled?: boolean
  appearance?: AccountAppearance
  transport?: Partial<AccountTransportInput>
  secret?: SecretInstructionInput
}

/** @spec docs/L1-api#account-crud-lifecycle */
export interface VerificationResponse {
  ok: boolean
  identityEmail: string | null
  pushSupported: boolean
}

/** @spec docs/L1-api#account-crud-lifecycle */
export interface StartProviderOAuthInput {
  provider: ProviderHint
  clientId: string
  clientSecret?: string
  redirectUri: string
}

/** @spec docs/L1-api#account-crud-lifecycle */
export interface StartOAuthResponse {
  authorizationUrl: string
  state: string
  redirectUri: string
}
