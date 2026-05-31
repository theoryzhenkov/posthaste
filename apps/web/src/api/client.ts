/**
 * Typed HTTP client for the PostHaste REST API.
 *
 * All functions target the `/v1` prefix. In Tauri, the backend port is
 * injected as `window.__POSTHASTE_PORT__` via initialization script. In
 * browser dev mode, falls back to `VITE_API_BASE_URL` or `localhost:3001`.
 *
 * @spec docs/L1-api#endpoint-table
 */
import { apiLogger } from '../logger'
import { LOG_EVENTS } from '../logEvents'
import {
  createOperationContext,
  createRequestContext,
  observabilityHeaders,
  type OperationContext,
} from '../observability'
import { getActiveConnection } from '../connection/runtime'
import { ApiError } from './errors'
import type {
  AccountOverview,
  ApiErrorCode,
  AppSettings,
  AutomationRulePreviewInput,
  AutomationRulePreviewResponse,
  CachedSenderAddress,
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
  ReadRequest,
  ReadResponse,
  ReplyContext,
  SendMessageInput,
  SmartMailbox,
  SmartMailboxSummary,
  StartOAuthResponse,
  StartProviderOAuthInput,
  SyncMode,
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
  operation?: OperationContext
}

type RequestInitWithObservability = RequestInit & {
  operation?: OperationContext
}

/**
 * The `/v1` base URL of the active connection. Dynamic (per active profile),
 * not a module-load const: reads the runtime holder, which is seeded to the
 * embedded injection (`__POSTHASTE_PORT__`, or the browser-dev fallback) so the
 * bundled build is unchanged, and re-pointed on a profile switch.
 *
 * @spec docs/eph/DESIGN-L1-deployment-modes#connection-profiles
 */
function baseUrl(): string {
  return getActiveConnection().baseUrl
}

/**
 * The per-connection bearer token. For the embedded profile this is the
 * injected `__POSTHASTE_TOKEN__` (today's behavior); for remote profiles it is
 * the keyring-held token. Absent when the server does not require auth.
 *
 * @spec docs/eph/DESIGN-L1-trust-model
 */
function authToken(): string | undefined {
  return getActiveConnection().token
}

/** The pinned `Host` header for a remote profile, or `undefined`. */
function hostHeader(): string | undefined {
  return getActiveConnection().hostHeader
}

/**
 * Per-request headers carrying the bearer token (and, for remote profiles, the
 * pinned `Host` header). Empty when no token is set.
 *
 * Exported because the SSE stream (via `fetchEventSource`) and the
 * browser-loadable blob fetches (logos, attachments) authenticate with the
 * same header set rather than a URL token — there is no `access_token` query
 * param anywhere anymore.
 */
export function authHeaders(): Record<string, string> {
  const headers: Record<string, string> = {}
  const token = authToken()
  if (token) {
    headers.Authorization = `Bearer ${token}`
  }
  const host = hostHeader()
  if (host) {
    headers.Host = host
  }
  return headers
}

export function buildMessageAttachmentUrl(
  sourceId: string,
  messageId: string,
  attachmentId: string,
): string {
  // No `?download=1` content-disposition variant: the bytes are loaded via an
  // authenticated blob fetch (preview) or saved with the `download` attribute
  // (downloads), so the client owns the filename and never needs the server to
  // force attachment disposition.
  return `${baseUrl()}/sources/${encodeURIComponent(sourceId)}/messages/${encodeURIComponent(messageId)}/attachments/${encodeURIComponent(attachmentId)}`
}

export function buildAccountLogoUrl(imageId: string): string {
  return `${baseUrl()}/account-assets/logos/${encodeURIComponent(imageId)}`
}

/** @spec docs/L1-api#account-crud-lifecycle */
export function buildOAuthRedirectUri(): string {
  return `${baseUrl()}/oauth/callback`
}

/** Parse a non-OK response into a structured {@link ApiError}. */
async function parseError(response: Response): Promise<never> {
  let message = response.statusText
  let code: ApiErrorCode | undefined

  try {
    const payload = (await response.json()) as {
      code?: ApiErrorCode
      message?: string
    }
    message = payload.message ?? message
    code = payload.code
  } catch {
    // Preserve the HTTP status text when the body is not JSON.
  }

  throw new ApiError(response.status, response.statusText, message, code)
}

function safeLogPath(path: string): string {
  return path.split('?')[0] ?? path
}

function mergeHeaders(
  base: HeadersInit | undefined,
  extra: HeadersInit,
): Headers {
  const headers = new Headers(base)
  for (const [key, value] of Object.entries(extra)) {
    headers.set(key, value)
  }
  return headers
}

/** Low-level fetch wrapper that throws {@link ApiError} on non-OK responses. */
async function request<T>(
  path: string,
  init: RequestInitWithObservability = {},
): Promise<T> {
  const { operation, headers, ...fetchInit } = init
  const context = createRequestContext(
    operation ?? createOperationContext('api.request', 'api-client'),
  )
  const requestPath = safeLogPath(path)
  const method = fetchInit.method ?? 'GET'
  const started = performance.now()
  apiLogger.debug(
    {
      event: LOG_EVENTS.apiRequestStarted,
      requestId: context.requestId,
      operationId: context.operationId,
      operationKind: context.operationKind,
      operationSource: context.operationSource,
      sessionId: context.sessionId,
      method,
      path: requestPath,
    },
    'api request started',
  )
  let response: Response
  try {
    response = await fetch(`${baseUrl()}${path}`, {
      ...fetchInit,
      headers: mergeHeaders(headers, {
        ...observabilityHeaders(context),
        ...authHeaders(),
      }),
    })
  } catch (error) {
    apiLogger.warn(
      {
        event: LOG_EVENTS.apiRequestFailed,
        requestId: context.requestId,
        operationId: context.operationId,
        operationKind: context.operationKind,
        operationSource: context.operationSource,
        sessionId: context.sessionId,
        method,
        path: requestPath,
        durationMs: Math.round(performance.now() - started),
        error,
      },
      'api request failed before response',
    )
    throw error
  }
  apiLogger.debug(
    {
      event: LOG_EVENTS.apiRequestCompleted,
      requestId: context.requestId,
      operationId: context.operationId,
      operationKind: context.operationKind,
      operationSource: context.operationSource,
      sessionId: context.sessionId,
      method,
      path: requestPath,
      status: response.status,
      durationMs: Math.round(performance.now() - started),
    },
    'api request completed',
  )
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
  operation?: OperationContext,
): Promise<T> {
  return request<T>(path, {
    method,
    headers: { 'Content-Type': 'application/json' },
    body: body === undefined ? undefined : JSON.stringify(body),
    operation,
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

/** @spec docs/L1-api#read-calls */
export async function read(request: ReadRequest): Promise<ReadResponse> {
  return jsonRequest<ReadResponse>('/read', 'POST', request)
}

/** @spec docs/L1-api#application-settings */
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

interface TriggerSyncInput {
  sourceId: string
  mode?: SyncMode
}

function normalizeTriggerSyncInput(
  input: string | TriggerSyncInput,
): TriggerSyncInput {
  return typeof input === 'string' ? { sourceId: input } : input
}

/** @spec docs/L1-api#endpoint-table */
export async function triggerSync(
  input: string | TriggerSyncInput,
): Promise<{ ok: boolean; eventCount: number; mode: SyncMode }> {
  const { sourceId, mode = 'incremental' } = normalizeTriggerSyncInput(input)
  return jsonRequest<{ ok: boolean; eventCount: number; mode: SyncMode }>(
    `/sources/${sourceId}/commands/sync`,
    'POST',
    { mode },
  )
}

/**
 * Build the SSE event stream URL, optionally resuming from a sequence number.
 * The stream is consumed via `fetchEventSource`, which authenticates with the
 * `Authorization` header (see {@link authHeaders}) — no token in the URL.
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
  return `${baseUrl()}/events${search ? `?${search}` : ''}`
}
