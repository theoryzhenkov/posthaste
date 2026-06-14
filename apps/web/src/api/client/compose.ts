import { jsonRequest, request } from './core'

import type {
  CachedSenderAddress,
  Identity,
  OkResponse,
  ReplyContext,
  SendMessageInput,
} from '../types'

/** @spec docs/L1-api#compose */
export async function fetchIdentity(sourceId: string): Promise<Identity> {
  return request<Identity>(`/sources/${sourceId}/identity`)
}

/** @spec docs/L1-api#compose */
export async function fetchSenderAddresses(): Promise<CachedSenderAddress[]> {
  return request<CachedSenderAddress[]>('/sender-addresses')
}

/** @spec docs/L1-api#compose */
export async function fetchReplyContext(
  sourceId: string,
  messageId: string,
): Promise<ReplyContext> {
  return request<ReplyContext>(
    `/sources/${sourceId}/messages/${messageId}/reply-context`,
  )
}

/** @spec docs/L1-api#compose */
export async function sendMessage(
  sourceId: string,
  input: SendMessageInput,
): Promise<OkResponse> {
  return jsonRequest<OkResponse>(
    `/sources/${sourceId}/commands/send`,
    'POST',
    input,
  )
}
