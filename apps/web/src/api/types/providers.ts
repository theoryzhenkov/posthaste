export type AccountDriver = 'jmap' | 'imapSmtp' | 'mock'

/** @spec docs/L0-providers#driver-model */
export type ProviderKind = 'generic' | 'gmail' | 'outlook' | 'icloud'

/** Compatibility alias for the existing serialized account setup field. */
export type ProviderHint = ProviderKind

/** @spec docs/L0-providers#authentication */
export type ProviderAuthKind = 'password' | 'appPassword' | 'oauth2'

/** @spec docs/L0-providers#imap-smtp-sync-strategy */
export type TransportSecurity = 'tls' | 'startTls' | 'plain'

/** @spec docs/L0-providers#imap-smtp-sync-strategy */
export interface MailEndpointSettings {
  host: string
  port: number
  security: TransportSecurity
}

/**
 * Redacted secret status returned by the API -- never contains the actual value.
 * @spec docs/L1-api#secret-management
 */
export interface SecretStatus {
  storage: 'env' | 'os'
  configured: boolean
  label: string | null
}
