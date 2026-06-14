import type {
  AccountConnectionOverview,
  AccountOverview,
  AccountTransportInput,
  CachedSenderAddress,
  CreateAccountInput,
  SecretInstructionInput,
  StartOAuthResponse,
  StartProviderOAuthInput,
  SyncProgress,
  UpdateAccountInput,
  VerificationResponse,
} from '../types'
import type { AssertTrue, Conforms, Wire } from './core'

export type _AccountConnectionOverview = AssertTrue<
  Conforms<AccountConnectionOverview, Wire['AccountConnectionOverview']>
>
export type _AccountOverview = AssertTrue<
  Conforms<AccountOverview, Wire['AccountOverview']>
>
export type _AccountTransportInput = AssertTrue<
  Conforms<AccountTransportInput, Wire['AccountTransportRequest']>
>
export type _SecretInstructionInput = AssertTrue<
  Conforms<SecretInstructionInput, Wire['SecretWriteRequest']>
>
export type _CreateAccountInput = AssertTrue<
  Conforms<CreateAccountInput, Wire['CreateAccountRequest']>
>
export type _UpdateAccountInput = AssertTrue<
  Conforms<UpdateAccountInput, Wire['PatchAccountRequest']>
>
export type _VerificationResponse = AssertTrue<
  Conforms<VerificationResponse, Wire['VerificationResponse']>
>
export type _StartProviderOAuthInput = AssertTrue<
  Conforms<StartProviderOAuthInput, Wire['StartProviderOAuthRequest']>
>
export type _StartOAuthResponse = AssertTrue<
  Conforms<StartOAuthResponse, Wire['StartOAuthResponse']>
>
export type _CachedSenderAddress = AssertTrue<
  Conforms<CachedSenderAddress, Wire['CachedSenderAddress']>
>
export type _SyncProgress = AssertTrue<
  Conforms<SyncProgress, Wire['SyncProgress']>
>
