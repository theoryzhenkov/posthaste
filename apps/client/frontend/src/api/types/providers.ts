export type AccountDriver = 'jmap' | 'imapSmtp' | 'mock'

export type ProviderKind = 'generic' | 'gmail' | 'outlook' | 'icloud'

/** Compatibility alias for the existing serialized account setup field. */
export type ProviderHint = ProviderKind

export type ProviderAuthKind = 'password' | 'appPassword' | 'oauth2'

export type TransportSecurity = 'tls' | 'startTls' | 'plain'

export interface MailEndpointSettings {
  host: string
  port: number
  security: TransportSecurity
}

/**
 * Redacted secret status returned by the API -- never contains the actual value.
 */
export interface SecretStatus {
  storage: 'env' | 'os'
  configured: boolean
  label: string | null
}
