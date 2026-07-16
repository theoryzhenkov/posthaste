import { jsonRequest, request } from './core'

import type { MessagePageInput } from './pagination'
import type {
  ConversationPage,
  CreateSmartMailboxInput,
  MessagePage,
  OkResponse,
  SmartMailbox,
  SmartMailboxSummary,
  UpdateSmartMailboxInput,
} from '../types'

/** @spec docs/L1-api#smart-mailbox-crud */
export async function fetchSmartMailboxes(): Promise<SmartMailboxSummary[]> {
  return request<SmartMailboxSummary[]>('/smart-mailboxes')
}

/** @spec docs/L1-api#smart-mailbox-crud */
export async function createSmartMailbox(
  input: CreateSmartMailboxInput,
): Promise<SmartMailbox> {
  return jsonRequest<SmartMailbox>('/smart-mailboxes', 'POST', input)
}

/** @spec docs/L1-api#smart-mailbox-crud */
export async function fetchSmartMailbox(id: string): Promise<SmartMailbox> {
  return request<SmartMailbox>(`/smart-mailboxes/${id}`)
}

/** @spec docs/L1-api#smart-mailbox-crud */
export async function updateSmartMailbox(
  id: string,
  input: UpdateSmartMailboxInput,
): Promise<SmartMailbox> {
  return jsonRequest<SmartMailbox>(`/smart-mailboxes/${id}`, 'PATCH', input)
}

/** @spec docs/L1-api#smart-mailbox-crud */
export async function deleteSmartMailbox(id: string): Promise<OkResponse> {
  return request<OkResponse>(`/smart-mailboxes/${id}`, { method: 'DELETE' })
}

/** @spec docs/L1-api#smart-mailbox-crud */
export async function resetDefaultSmartMailboxes(): Promise<
  SmartMailboxSummary[]
> {
  return request<SmartMailboxSummary[]>('/smart-mailboxes:reset-defaults', {
    method: 'POST',
  })
}

/** @spec docs/L1-api#endpoint-table */
export async function fetchSmartMailboxMessages(
  id: string,
  input?: MessagePageInput,
): Promise<MessagePage> {
  const params = new URLSearchParams()
  if (input?.limit !== undefined) {
    params.set('limit', String(input.limit))
  }
  if (input?.cursor) {
    params.set('cursor', input.cursor)
  }
  if (input?.sort) {
    params.set('sort', input.sort)
  }
  if (input?.sortDir) {
    params.set('sortDir', input.sortDir)
  }
  if (input?.q) {
    params.set('q', input.q)
  }
  const search = params.toString()
  return request<MessagePage>(
    `/smart-mailboxes/${id}/messages${search ? `?${search}` : ''}`,
    { signal: input?.signal, operation: input?.operation },
  )
}

/**
 * Fetch a cursor-paginated page of conversations for a smart mailbox.
 * @spec docs/L1-api#cursor-pagination
 */
export async function fetchSmartMailboxConversations(
  id: string,
  input?: {
    limit?: number
    cursor?: string | null
    sort?: string
    sortDir?: string
    q?: string
  },
): Promise<ConversationPage> {
  const params = new URLSearchParams()
  if (input?.limit !== undefined) {
    params.set('limit', String(input.limit))
  }
  if (input?.cursor) {
    params.set('cursor', input.cursor)
  }
  if (input?.sort) {
    params.set('sort', input.sort)
  }
  if (input?.sortDir) {
    params.set('sortDir', input.sortDir)
  }
  if (input?.q) {
    params.set('q', input.q)
  }
  const search = params.toString()
  return request<ConversationPage>(
    `/smart-mailboxes/${id}/conversations${search ? `?${search}` : ''}`,
  )
}
