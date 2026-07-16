import type {
  AccountAppearance,
  AppSettings,
  CachePolicy,
  MailEndpointSettings,
  SecretStatus,
  TagAppearance,
} from '../types'
import type { AssertTrue, Conforms, Wire } from './core'

export type _MailEndpointSettings = AssertTrue<
  Conforms<MailEndpointSettings, Wire['ImapTransportSettings']>
>
// Client appearance preferences live behind the frontend ClientPreferencesStore
// boundary. They are intentionally NOT in the daemon wire schema, so they have
// no conformance assertion here.
export type _AppSettings = AssertTrue<
  Conforms<AppSettings, Wire['AppSettings']>
>
export type _TagAppearance = AssertTrue<
  Conforms<TagAppearance, Wire['TagAppearance']>
>
export type _CachePolicy = AssertTrue<
  Conforms<CachePolicy, Wire['CachePolicy']>
>
export type _SecretStatus = AssertTrue<
  Conforms<SecretStatus, Wire['SecretStatus']>
>
export type _AccountAppearance = AssertTrue<
  Conforms<AccountAppearance, Wire['AccountAppearance']>
>
