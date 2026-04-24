/**
 * Typed HTTP client for the PostHaste REST API.
 *
 * All functions target the `/v1` prefix. In Tauri, the backend port is
 * injected as `window.__POSTHASTE_PORT__` via initialization script. In
 * browser dev mode, falls back to `VITE_API_BASE_URL` or `localhost:3001`.
 *
 * @spec docs/L1-api#endpoint-table
 */
import { ApiError } from './errors'
import type {
  AccountOverview,
  AppSettings,
  AutomationRulePreviewInput,
  AutomationRulePreviewResponse,
  ConversationPage,
  ConversationView,
  CreateAccountInput,
  CreateSmartMailboxInput,
  Identity,
  Mailbox,
  MessageCommand,
  MessageCommandResult,
  MessageDetail,
  MessagePage,
  MessageSortField,
  OkResponse,
  PatchMailboxInput,
  ReplyContext,
  SendMessageInput,
  SidebarResponse,
  SmartMailbox,
  SmartMailboxSummary,
  UpdateAccountInput,
  UpdateSmartMailboxInput,
  VerificationResponse,
} from './types'

interface MessagePageInput {
  q?: string
  limit?: number
  cursor?: string | null
  sort?: MessageSortField
  sortDir?: string
  signal?: AbortSignal
}

function normalizeApiBaseUrl(baseUrl: string): string {
  return baseUrl.replace(/\/+$/, '')
}

function resolveBaseUrl(): string {
  const port = (window as unknown as Record<string, unknown>).__POSTHASTE_PORT__
  if (typeof port === 'number') {
    return `http://127.0.0.1:${port}/v1`
  }
  return normalizeApiBaseUrl(
    import.meta.env.VITE_API_BASE_URL?.trim() || 'http://localhost:3001/v1',
  )
}

const BASE_URL = resolveBaseUrl()

export function buildMessageAttachmentUrl(
  sourceId: string,
  messageId: string,
  attachmentId: string,
  options?: { download?: boolean },
): string {
  const url = new URL(
    `${BASE_URL}/sources/${encodeURIComponent(sourceId)}/messages/${encodeURIComponent(messageId)}/attachments/${encodeURIComponent(attachmentId)}`,
  )
  if (options?.download) {
    url.searchParams.set('download', '1')
  }
  return url.toString()
}

export function buildAccountLogoUrl(imageId: string): string {
  return `${BASE_URL}/account-assets/logos/${encodeURIComponent(imageId)}`
}

/** Parse a non-OK response into a structured {@link ApiError}. */
async function parseError(response: Response): Promise<never> {
  let message = response.statusText
  let code: string | undefined

  try {
    const payload = (await response.json()) as {
      code?: string
      message?: string
    }
    message = payload.message ?? message
    code = payload.code
  } catch {
    // Preserve the HTTP status text when the body is not JSON.
  }

  throw new ApiError(response.status, response.statusText, message, code)
}

/** Low-level fetch wrapper that throws {@link ApiError} on non-OK responses. */
async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const response = await fetch(`${BASE_URL}${path}`, init)
  if (!response.ok) {
    return parseError(response)
  }
  return response.json() as Promise<T>
}

/** Convenience wrapper for JSON-bodied requests (POST / PATCH). */
function jsonRequest<T>(
  path: string,
  method: string,
  body?: unknown,
): Promise<T> {
  return request<T>(path, {
    method,
    headers: { 'Content-Type': 'application/json' },
    body: body === undefined ? undefined : JSON.stringify(body),
  })
}

/** @spec docs/L1-api#endpoint-table */
export async function fetchSettings(): Promise<AppSettings> {
  return request<AppSettings>('/settings')
}

/** @spec docs/L1-api#endpoint-table */
export async function patchSettings(
  input: Partial<AppSettings>,
): Promise<AppSettings> {
  return jsonRequest<AppSettings>('/settings', 'PATCH', input)
}

/** @spec docs/L1-api#account-crud-lifecycle */
export async function previewAutomationRule(
  input: AutomationRulePreviewInput,
): Promise<AutomationRulePreviewResponse> {
  return jsonRequest<AutomationRulePreviewResponse>(
    '/automation-rules:preview',
    'POST',
    input,
  )
}

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

/** @spec docs/L1-api#endpoint-table */
export async function fetchMailboxes(accountId: string): Promise<Mailbox[]> {
  return request<Mailbox[]>(
    `/sources/${encodeURIComponent(accountId)}/mailboxes`,
  )
}

/** @spec docs/L1-api#endpoint-table */
export async function patchMailbox(
  accountId: string,
  mailboxId: string,
  input: PatchMailboxInput,
): Promise<Mailbox[]> {
  return jsonRequest<Mailbox[]>(
    `/sources/${encodeURIComponent(accountId)}/mailboxes/${encodeURIComponent(mailboxId)}`,
    'PATCH',
    input,
  )
}

/** @spec docs/L1-api#endpoint-table */
export async function fetchSidebar(): Promise<SidebarResponse> {
  return request<SidebarResponse>('/sidebar')
}

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
    { signal: input?.signal },
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
    { signal: input?.signal },
  )
}

/** @spec docs/L1-api#compose */
export async function fetchIdentity(sourceId: string): Promise<Identity> {
  return request<Identity>(`/sources/${sourceId}/identity`)
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

/** @spec docs/L1-api#endpoint-table */
export async function triggerSync(
  sourceId: string,
): Promise<{ ok: boolean; eventCount: number }> {
  return request<{ ok: boolean; eventCount: number }>(
    `/sources/${sourceId}/commands/sync`,
    { method: 'POST' },
  )
}

/**
 * Build the SSE event stream URL, optionally resuming from a sequence number.
 * @spec docs/L1-api#sse-event-stream
 */
export function buildEventsUrl(input?: {
  accountId?: string
  afterSeq?: number | null
}): string {
  const params = new URLSearchParams()
  if (input?.accountId) {
    params.set('accountId', input.accountId)
  }
  if (input?.afterSeq != null) {
    params.set('afterSeq', String(input.afterSeq))
  }
  const search = params.toString()
  return `${BASE_URL}/events${search ? `?${search}` : ''}`
}
