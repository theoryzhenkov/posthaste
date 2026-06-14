import { jsonRequest, request } from './core'

import type { MessagePageInput } from './pagination'
import type {
  ConversationPage,
  ConversationView,
  MessageCommand,
  MessageCommandResult,
  MessageDetail,
  MessagePage,
} from '../types'

/**
 * Fetch a cursor-paginated page of conversations, optionally filtered by source or mailbox.
 * @spec docs/L1-api#cursor-pagination
 */
export async function fetchConversations(input?: {
  sourceId?: string | null
  mailboxId?: string | null
  limit?: number
  cursor?: string | null
  sort?: string
  sortDir?: string
  q?: string
}): Promise<ConversationPage> {
  const params = new URLSearchParams()
  if (input?.sourceId) {
    params.set('sourceId', input.sourceId)
  }
  if (input?.mailboxId) {
    params.set('mailboxId', input.mailboxId)
  }
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
    `/views/conversations${search ? `?${search}` : ''}`,
  )
}

/** @spec docs/L1-api#endpoint-table */
export async function fetchConversation(
  conversationId: string,
): Promise<ConversationView> {
  return request<ConversationView>(`/views/conversations/${conversationId}`)
}

/**
 * Fetch full message detail (body is sanitized in Rust before reaching the response).
 * @spec docs/L1-api#message-body-sanitization
 */
export async function fetchMessage(
  messageId: string,
  sourceId: string,
): Promise<MessageDetail> {
  return request<MessageDetail>(`/sources/${sourceId}/messages/${messageId}`)
}

/** @spec docs/L1-api#endpoint-table */
export async function fetchSourceMessages(
  sourceId: string,
  mailboxId: string | null,
  input?: MessagePageInput,
): Promise<MessagePage> {
  const params = new URLSearchParams()
  if (mailboxId) {
    params.set('mailboxId', mailboxId)
  }
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
    `/sources/${sourceId}/messages${search ? `?${search}` : ''}`,
    { signal: input?.signal, operation: input?.operation },
  )
}

/** @spec docs/L1-api#endpoint-table */
export async function fetchSearchMessages(
  q: string,
  input?: Omit<MessagePageInput, 'q'>,
): Promise<MessagePage> {
  const params = new URLSearchParams({ q })
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
  return request<MessagePage>(`/messages/search?${params.toString()}`, {
    signal: input?.signal,
    operation: input?.operation,
  })
}

/**
 * Dispatch a message command (keyword change, mailbox move, or destroy).
 * @spec docs/L1-api#endpoint-table
 */
export async function performMessageCommand(
  messageId: string,
  command: MessageCommand,
  sourceId: string,
): Promise<MessageCommandResult> {
  switch (command.kind) {
    case 'setKeywords':
      return jsonRequest<MessageCommandResult>(
        `/sources/${sourceId}/commands/messages/${messageId}/set-keywords`,
        'POST',
        {
          add: command.add,
          remove: command.remove,
        },
      )
    case 'addToMailbox':
      return jsonRequest<MessageCommandResult>(
        `/sources/${sourceId}/commands/messages/${messageId}/add-to-mailbox`,
        'POST',
        { mailboxId: command.mailboxId },
      )
    case 'removeFromMailbox':
      return jsonRequest<MessageCommandResult>(
        `/sources/${sourceId}/commands/messages/${messageId}/remove-from-mailbox`,
        'POST',
        { mailboxId: command.mailboxId },
      )
    case 'replaceMailboxes':
      return jsonRequest<MessageCommandResult>(
        `/sources/${sourceId}/commands/messages/${messageId}/replace-mailboxes`,
        'POST',
        { mailboxIds: command.mailboxIds },
      )
    case 'destroy':
      return request<MessageCommandResult>(
        `/sources/${sourceId}/commands/messages/${messageId}/destroy`,
        {
          method: 'POST',
        },
      )
  }
}
