import { jsonRequest, request } from './core'

import type {
  AccountOverview,
  CreateAccountInput,
  OkResponse,
  StartOAuthResponse,
  StartProviderOAuthInput,
  UpdateAccountInput,
  VerificationResponse,
} from '../types'

/** @spec docs/L1-api#endpoint-table */
export async function fetchAccounts(): Promise<AccountOverview[]> {
  return request<AccountOverview[]>('/accounts')
}

/** @spec docs/L1-api#endpoint-table */
export async function fetchAccount(
  accountId: string,
): Promise<AccountOverview> {
  return request<AccountOverview>(`/accounts/${accountId}`)
}

/** @spec docs/L1-api#account-crud-lifecycle */
export async function createAccount(
  input: CreateAccountInput,
): Promise<AccountOverview> {
  return jsonRequest<AccountOverview>('/accounts', 'POST', input)
}

/**
 * Sparse-merge update -- omitted fields are preserved on the backend.
 * @spec docs/L1-api#account-crud-lifecycle
 */
export async function updateAccount(
  accountId: string,
  input: UpdateAccountInput,
): Promise<AccountOverview> {
  return jsonRequest<AccountOverview>(`/accounts/${accountId}`, 'PATCH', input)
}

/** @spec docs/L1-api#account-crud-lifecycle */
export async function uploadAccountLogo(
  accountId: string,
  file: File,
): Promise<AccountOverview> {
  return request<AccountOverview>(`/accounts/${accountId}/logo`, {
    method: 'POST',
    headers: {
      'Content-Type': file.type || 'application/octet-stream',
    },
    body: file,
  })
}

/** @spec docs/L1-api#account-crud-lifecycle */
export async function deleteAccount(accountId: string): Promise<OkResponse> {
  return request<OkResponse>(`/accounts/${accountId}`, { method: 'DELETE' })
}

/** @spec docs/L1-api#account-crud-lifecycle */
export async function verifyAccount(
  accountId: string,
): Promise<VerificationResponse> {
  return request<VerificationResponse>(`/accounts/${accountId}/verify`, {
    method: 'POST',
  })
}

/** @spec docs/L1-api#account-crud-lifecycle */
export async function startProviderOAuth(
  input: StartProviderOAuthInput,
): Promise<StartOAuthResponse> {
  return jsonRequest<StartOAuthResponse>('/oauth/start', 'POST', input)
}

/** @spec docs/L1-api#account-crud-lifecycle */
export async function enableAccount(accountId: string): Promise<OkResponse> {
  return request<OkResponse>(`/accounts/${accountId}/enable`, {
    method: 'POST',
  })
}

/** @spec docs/L1-api#account-crud-lifecycle */
export async function disableAccount(accountId: string): Promise<OkResponse> {
  return request<OkResponse>(`/accounts/${accountId}/disable`, {
    method: 'POST',
  })
}
