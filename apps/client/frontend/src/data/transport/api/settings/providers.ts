// The closed provider vocabularies are wire-borne: re-served from `gen/`
// (ts-rs output), never restated here.
export type {
  AccountDriver,
  ProviderAuthKind,
  ProviderHint,
  TransportSecurity,
} from '@/gen'
import type { ProviderHint, TransportSecurity } from '@/gen'

/** Compatibility alias: the UI historically names the wire's `ProviderHint`
 *  set `ProviderKind`. */
export type ProviderKind = ProviderHint

export interface MailEndpointSettings {
  host: string
  port: number
  security: TransportSecurity
}

/** Redacted secret status returned by the API — never the actual value.
 *  Wire shape, re-served from `gen/`. */
export type { SecretStatus } from '@/gen'
