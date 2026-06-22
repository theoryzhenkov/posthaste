import { jsonRequest, request } from './core'

import type {
  CachedSenderAddress,
  DraftContent,
  Identity,
  OkResponse,
  Operation,
  ReplyContext,
  SaveDraftInput,
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

/** @spec docs/L1-outbox#operation-model */
export async function fetchDraftContent(
  sourceId: string,
  messageId: string,
): Promise<DraftContent> {
  return request<DraftContent>(
    `/sources/${sourceId}/messages/${messageId}/draft-content`,
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

/** @spec docs/L1-outbox#operation-model */
export async function saveDraft(
  sourceId: string,
  input: SaveDraftInput,
): Promise<Operation> {
  return jsonRequest<Operation>(
    `/sources/${sourceId}/commands/save-draft`,
    'POST',
    input,
  )
}

/** @spec docs/L1-outbox#operation-model */
export async function deleteDraft(
  sourceId: string,
  draftId: string,
): Promise<Operation> {
  return jsonRequest<Operation>(
    `/sources/${sourceId}/commands/delete-draft`,
    'POST',
    { draftId },
  )
}

/** @spec docs/L1-outbox#operation-model */
export async function listPendingOperations(
  sourceId: string,
): Promise<Operation[]> {
  return request<Operation[]>(`/sources/${sourceId}/operations`)
}

/** @spec docs/L1-outbox#operation-model */
export async function discardOperation(
  sourceId: string,
  operationId: string,
): Promise<void> {
  await jsonRequest<{ ok: true }>(
    `/sources/${sourceId}/operations/${encodeURIComponent(operationId)}`,
    'DELETE',
  )
}

/** @spec docs/L1-outbox#operation-model */
export async function retryOperation(
  sourceId: string,
  operationId: string,
): Promise<void> {
  await jsonRequest<{ ok: true }>(
    `/sources/${sourceId}/operations/${encodeURIComponent(operationId)}/retry`,
    'POST',
  )
}
