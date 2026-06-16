import type {
  AccountOverview,
  CreateAccountInput,
  OkResponse,
  StartOAuthResponse,
  StartProviderOAuthInput,
  UpdateAccountInput,
  VerificationResponse,
} from '../api/types'
import { getRuntimeAdapter } from './adapter'

export function fetchRuntimeAccount(
  accountId: string,
): Promise<AccountOverview> {
  return getRuntimeAdapter().fetchAccount(accountId)
}

export function createRuntimeAccount(
  input: CreateAccountInput,
): Promise<AccountOverview> {
  return getRuntimeAdapter().createAccount(input)
}

export function updateRuntimeAccount(
  accountId: string,
  input: UpdateAccountInput,
): Promise<AccountOverview> {
  return getRuntimeAdapter().updateAccount(accountId, input)
}

export function uploadRuntimeAccountLogo(
  accountId: string,
  file: File,
): Promise<AccountOverview> {
  return getRuntimeAdapter().uploadAccountLogo(accountId, file)
}

export function verifyRuntimeAccount(
  accountId: string,
): Promise<VerificationResponse> {
  return getRuntimeAdapter().verifyAccount(accountId)
}

export function enableRuntimeAccount(accountId: string): Promise<OkResponse> {
  return getRuntimeAdapter().enableAccount(accountId)
}

export function disableRuntimeAccount(accountId: string): Promise<OkResponse> {
  return getRuntimeAdapter().disableAccount(accountId)
}

export function deleteRuntimeAccount(accountId: string): Promise<OkResponse> {
  return getRuntimeAdapter().deleteAccount(accountId)
}

export function fetchRuntimeOAuthRedirectUri(): string {
  return getRuntimeAdapter().fetchOAuthRedirectUri()
}

export function startRuntimeProviderOAuth(
  input: StartProviderOAuthInput,
): Promise<StartOAuthResponse> {
  return getRuntimeAdapter().startProviderOAuth(input)
}
