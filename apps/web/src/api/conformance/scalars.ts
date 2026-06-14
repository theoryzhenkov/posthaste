import type {
  AccountDriver,
  AutomationTrigger,
  MessageSortField,
  ProviderAuthKind,
  ProviderHint,
  ProviderKind,
  SmartMailboxField,
  SmartMailboxGroupOperator,
  SmartMailboxKind,
  SmartMailboxOperator,
  SmartMailboxValue,
  SyncMode,
  TransportSecurity,
} from '../types'
import type { AssertTrue, Conforms, Wire } from './core'

export type _AccountDriver = AssertTrue<
  Conforms<AccountDriver, Wire['AccountDriver']>
>
export type _ProviderKind = AssertTrue<
  Conforms<ProviderKind, Wire['ProviderKind']>
>
export type _ProviderHint = AssertTrue<
  Conforms<ProviderHint, Wire['ProviderHint']>
>
export type _ProviderAuthKind = AssertTrue<
  Conforms<ProviderAuthKind, Wire['ProviderAuthKind']>
>
export type _TransportSecurity = AssertTrue<
  Conforms<TransportSecurity, Wire['TransportSecurity']>
>
export type _AutomationTrigger = AssertTrue<
  Conforms<AutomationTrigger, Wire['AutomationTrigger']>
>
export type _MessageSortField = AssertTrue<
  Conforms<MessageSortField, Wire['MessageSortField']>
>
export type _SyncMode = AssertTrue<Conforms<SyncMode, Wire['SyncMode']>>
export type _SmartMailboxKind = AssertTrue<
  Conforms<SmartMailboxKind, Wire['SmartMailboxKind']>
>
export type _SmartMailboxGroupOperator = AssertTrue<
  Conforms<SmartMailboxGroupOperator, Wire['SmartMailboxGroupOperator']>
>
export type _SmartMailboxField = AssertTrue<
  Conforms<SmartMailboxField, Wire['SmartMailboxField']>
>
export type _SmartMailboxOperator = AssertTrue<
  Conforms<SmartMailboxOperator, Wire['SmartMailboxOperator']>
>
export type _SmartMailboxValue = AssertTrue<
  Conforms<SmartMailboxValue, Wire['SmartMailboxValue']>
>
