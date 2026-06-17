import type {
  AccountOverview,
  CreateAccountInput,
  OkResponse,
  StartOAuthResponse,
  StartProviderOAuthInput,
  UpdateAccountInput,
  VerificationResponse,
} from '../api/types'
import { runtimeMutations } from './mutations'
import { runtimeViews } from './views'

export function fetchRuntimeAccount(
  accountId: string,
): Promise<AccountOverview> {
  return runtimeViews.accounts.detail(accountId)
}

export function createRuntimeAccount(
  input: CreateAccountInput,
): Promise<AccountOverview> {
  return runtimeMutations.accounts.create(input)
}

export function updateRuntimeAccount(
  accountId: string,
  input: UpdateAccountInput,
): Promise<AccountOverview> {
  return runtimeMutations.accounts.update(accountId, input)
}

export function uploadRuntimeAccountLogo(
  accountId: string,
  file: File,
): Promise<AccountOverview> {
  return runtimeMutations.accounts.uploadLogo(accountId, file)
}

export function verifyRuntimeAccount(
  accountId: string,
): Promise<VerificationResponse> {
  return runtimeMutations.accounts.verify(accountId)
}

export function enableRuntimeAccount(accountId: string): Promise<OkResponse> {
  return runtimeMutations.accounts.enable(accountId)
}

export function disableRuntimeAccount(accountId: string): Promise<OkResponse> {
  return runtimeMutations.accounts.disable(accountId)
}

export function deleteRuntimeAccount(accountId: string): Promise<OkResponse> {
  return runtimeMutations.accounts.delete(accountId)
}

export function fetchRuntimeOAuthRedirectUri(): string {
  return runtimeViews.oauth.redirectUri()
}

export function startRuntimeProviderOAuth(
  input: StartProviderOAuthInput,
): Promise<StartOAuthResponse> {
  return runtimeMutations.oauth.startProvider(input)
}
