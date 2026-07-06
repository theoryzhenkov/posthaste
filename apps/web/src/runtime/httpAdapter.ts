import {
  authHeaders,
  buildAccountLogoUrl,
  buildMessageAttachmentUrl,
  buildMessageBodyUrl,
  buildOAuthRedirectUri,
  closeRuntimeLink,
  closeRuntimeLinkView,
  createAccount,
  createMailbox,
  createRule,
  createSmartMailbox,
  deleteAccount,
  deleteRule,
  deleteSmartMailbox,
  disableAccount,
  enableAccount,
  fetchAccount,
  fetchAccounts,
  fetchConversation,
  fetchConversations,
  fetchIdentity,
  fetchMailboxes,
  fetchMessage,
  fetchDraftContent,
  fetchReplyContext,
  fetchRules,
  fetchSearchMessages,
  fetchSenderAddresses,
  fetchSettings,
  fetchSmartMailbox,
  fetchSmartMailboxMessages,
  fetchSmartMailboxes,
  fetchSourceMessages,
  extendRuntimeLinkView,
  openRuntimeLinkView,
  patchMailbox,
  patchSettings,
  performMessageCommand,
  previewAutomationRule,
  read,
  resetDefaultSmartMailboxes,
  saveDraft,
  deleteDraft,
  discardOperation,
  listPendingOperations,
  retryOperation,
  sendMessage,
  startProviderOAuth,
  triggerSync,
  updateAccount,
  updateRule,
  updateSmartMailbox,
  uploadAccountLogo,
  verifyAccount,
} from '../api/client'

import type { KnownMailboxRole, Mailbox } from '../api/types'

import type {
  RuntimeAdapter,
  RuntimeMailListViewState,
  RuntimeMailQueryRequest,
  RuntimeMessagePageRequest,
  RuntimeResourceDescriptor,
  RuntimeViewDescriptor,
  RuntimeViewSnapshot,
} from './types'
import { queryClient as singletonQueryClient } from '@/app/queryClient'
import {
  buildMailListPredicateContext,
  isMailListSelfMaintained,
} from './mailListSelfMaintained'
import {
  connectNearEnd,
  disconnectNearEnd,
  forwardNearEndMutation,
  subscribeNearEndFrames,
} from './nearEnd'

/**
 * Default runtime adapter during migration.
 *
 * It preserves production behavior by delegating to the existing typed HTTP
 * client while renderer code moves behind the runtime facade.
 */
function currentBackendSort(sort: RuntimeMessagePageRequest['sort']) {
  return sort === 'relevance' ? undefined : sort
}

function runtimeSort(sort: RuntimeMessagePageRequest['sort']) {
  return !sort || sort === 'relevance' ? 'date' : sort
}

function scopeQuery(request: RuntimeMessagePageRequest): string {
  const parts: string[] = []
  switch (request.scope.kind) {
    case 'source-mailbox':
      parts.push(
        `in:${request.scope.sourceId}/${request.scope.mailboxId ?? ''}`,
      )
      break
    case 'smart-mailbox':
      parts.push(`in:${request.scope.smartMailboxId}`)
      break
    case 'global':
      break
  }
  const userQuery = request.query?.trim()
  if (userQuery) {
    parts.push(userQuery)
  }
  return parts.join(' ')
}

function mailQueryRequest(
  request: RuntimeMessagePageRequest,
): RuntimeMailQueryRequest {
  return {
    query: scopeQuery(request),
    presentation: {
      kind: 'messages',
      limit: request.limit,
      cursor: null,
      sortField: runtimeSort(request.sort),
      sortDirection: request.sortDir ?? 'desc',
    },
    visibility: null,
  }
}

/** Build the mailList view descriptor, stamping `clientSelfMaintained` from the
 * view's scope+sort (single source: `isMailListSelfMaintained`, shared with the
 * store's predicate derivation). The runtime reads the flag to decide whether to
 * skip the per-event re-serve (option iii); a `Deferred` mail-list (smart-
 * mailbox / global / non-`date`) stays false and is re-served. */
function mailListViewDescriptor(
  view: RuntimeMessagePageRequest,
): RuntimeViewDescriptor {
  return {
    family: 'mailList',
    payload: mailQueryRequest(view),
    clientSelfMaintained: isMailListSelfMaintained(
      view.scope,
      view.sort,
      buildMailListPredicateContext(singletonQueryClient),
    ),
  }
}

function requiredMailboxByRole(
  mailboxes: Mailbox[],
  sourceId: string,
  role: KnownMailboxRole,
): Mailbox {
  const mailbox = mailboxes.find((candidate) => candidate.role === role)
  if (!mailbox) {
    throw new Error(`Missing mailbox with role ${role} for source ${sourceId}`)
  }
  return mailbox
}

function resourceUrl(resource: RuntimeResourceDescriptor): string {
  switch (resource.kind) {
    case 'account-logo':
      return buildAccountLogoUrl(resource.imageId)
    case 'message-attachment':
      return buildMessageAttachmentUrl(
        resource.sourceId,
        resource.messageId,
        resource.attachmentId,
      )
    case 'message-body':
      return buildMessageBodyUrl(
        resource.sourceId,
        resource.messageId,
        resource.format,
      )
  }
}

export const httpRuntimeAdapter: RuntimeAdapter = {
  // The link lifecycle, frame stream, and mutation forward ride the shared
  // near-end ENGINE (wasm, D41): link open, reconnect + resume cursor,
  // deadlines, backoff, typed frame parse and 4xx classification all live in
  // the engine — this adapter carries zero transport policy for them.
  openRuntimeLink(request) {
    // `viewDelta` is engine config (always on for engine links).
    return connectNearEnd({ sourceId: request.sourceId })
  },
  async closeRuntimeLink(request) {
    // Stop the engine's frame loop, then close the link server-side (a
    // policy-free DELETE — the engine's transport is post/stream only).
    await disconnectNearEnd()
    return closeRuntimeLink(request.linkId, {
      sourceId: request.sourceId,
    })
  },
  async openRuntimeLinkMessageListView(request) {
    const descriptor = mailListViewDescriptor(request.view)
    return openRuntimeLinkView<RuntimeViewSnapshot<RuntimeMailListViewState>>(
      request.linkId,
      { descriptor },
      { sourceId: request.sourceId },
    )
  },
  openRuntimeLinkView(request) {
    return openRuntimeLinkView(
      request.linkId,
      { descriptor: request.descriptor },
      { sourceId: request.sourceId },
    )
  },
  extendRuntimeLinkView(request) {
    return extendRuntimeLinkView<RuntimeViewSnapshot<RuntimeMailListViewState>>(
      request.linkId,
      request.viewId,
      request.count,
      {
        sourceId: request.sourceId,
      },
    )
  },
  closeRuntimeLinkView(request) {
    return closeRuntimeLinkView(request.linkId, request.viewId, {
      sourceId: request.sourceId,
    })
  },
  runRuntimeMutation(request) {
    // The engine stamps its own link id, applies the request deadline,
    // retries transients with jittered backoff, and parses the receipt typed.
    return forwardNearEndMutation(request)
  },
  subscribeRuntimeFrames(_request, handlers) {
    // Frames arrive already parsed + validated by the engine; the reconnect
    // loop and the `afterSeq` resume cursor are engine-owned (callers no
    // longer thread `afterSeq`).
    return subscribeNearEndFrames(handlers)
  },
  createAccount(input) {
    return createAccount(input)
  },
  createSmartMailbox(input) {
    return createSmartMailbox(input)
  },
  deleteAccount(accountId) {
    return deleteAccount(accountId)
  },
  deleteSmartMailbox(smartMailboxId) {
    return deleteSmartMailbox(smartMailboxId)
  },
  disableAccount(accountId) {
    return disableAccount(accountId)
  },
  enableAccount(accountId) {
    return enableAccount(accountId)
  },
  fetchAccount(accountId) {
    return fetchAccount(accountId)
  },
  fetchAccounts() {
    return fetchAccounts()
  },
  fetchConversation(conversationId) {
    return fetchConversation(conversationId)
  },
  fetchConversationPage(request) {
    return fetchConversations(request)
  },
  fetchIdentity(sourceId) {
    return fetchIdentity(sourceId)
  },
  fetchMailboxes(accountId) {
    return fetchMailboxes(accountId)
  },
  fetchMessage(messageId, sourceId) {
    return fetchMessage(messageId, sourceId)
  },
  async fetchResourceBlob(descriptor, options) {
    const response = await fetch(resourceUrl(descriptor), {
      headers: authHeaders(),
      signal: options?.signal,
    })
    if (!response.ok) {
      throw new Error(`resource fetch failed with ${response.status}`)
    }
    return response.blob()
  },
  fetchMessagePage(req) {
    const input = {
      q: req.query,
      cursor: req.cursor,
      limit: req.limit,
      sort: currentBackendSort(req.sort),
      sortDir: req.sortDir,
      signal: req.signal,
      operation: req.operation,
    }

    switch (req.scope.kind) {
      case 'source-mailbox':
        return fetchSourceMessages(
          req.scope.sourceId,
          req.scope.mailboxId,
          input,
        )
      case 'smart-mailbox':
        return fetchSmartMailboxMessages(req.scope.smartMailboxId, input)
      case 'global':
        if (!req.query?.trim()) {
          return Promise.resolve({ items: [], nextCursor: null })
        }
        return fetchSearchMessages(req.query, input)
    }
  },
  fetchOAuthRedirectUri() {
    return buildOAuthRedirectUri()
  },
  fetchReplyContext({ sourceId, messageId }) {
    return fetchReplyContext(sourceId, messageId)
  },
  fetchDraftContent({ sourceId, messageId }) {
    return fetchDraftContent(sourceId, messageId)
  },
  fetchSenderAddresses() {
    return fetchSenderAddresses()
  },
  fetchSettings() {
    return fetchSettings()
  },
  fetchSmartMailbox(smartMailboxId) {
    return fetchSmartMailbox(smartMailboxId)
  },
  fetchSmartMailboxes() {
    return fetchSmartMailboxes()
  },
  patchMailbox(accountId, mailboxId, input) {
    return patchMailbox(accountId, mailboxId, input)
  },
  createMailbox(accountId, input) {
    return createMailbox(accountId, input)
  },
  patchSettings(input) {
    return patchSettings(input)
  },
  previewAutomationRule(input) {
    return previewAutomationRule(input)
  },
  read(request) {
    return read(request)
  },
  fetchRules() {
    return fetchRules()
  },
  createRule(input) {
    return createRule(input)
  },
  updateRule(id, input) {
    return updateRule(id, input)
  },
  deleteRule(id) {
    return deleteRule(id)
  },
  runMessageCommand({ command, messageId, sourceId }) {
    return performMessageCommand(messageId, command, sourceId)
  },
  async moveMessageToMailboxRole({ messageId, role, sourceId }) {
    const mailboxes = await fetchMailboxes(sourceId)
    const mailbox = requiredMailboxByRole(mailboxes, sourceId, role)
    return performMessageCommand(
      messageId,
      { kind: 'replaceMailboxes', mailboxIds: [mailbox.id] },
      sourceId,
    )
  },
  resetDefaultSmartMailboxes() {
    return resetDefaultSmartMailboxes()
  },
  sendMessage({ sourceId, input }) {
    return sendMessage(sourceId, input)
  },
  saveDraft({ sourceId, input }) {
    return saveDraft(sourceId, input)
  },
  deleteDraft({ sourceId, draftId }) {
    return deleteDraft(sourceId, draftId)
  },
  listPendingOperations(sourceId) {
    return listPendingOperations(sourceId)
  },
  discardOperation(sourceId, operationId) {
    return discardOperation(sourceId, operationId)
  },
  retryOperation(sourceId, operationId) {
    return retryOperation(sourceId, operationId)
  },
  startProviderOAuth(input) {
    return startProviderOAuth(input)
  },
  triggerSync(request) {
    return triggerSync(request)
  },
  updateAccount(accountId, input) {
    return updateAccount(accountId, input)
  },
  updateSmartMailbox(smartMailboxId, input) {
    return updateSmartMailbox(smartMailboxId, input)
  },
  uploadAccountLogo(accountId, file) {
    return uploadAccountLogo(accountId, file)
  },
  verifyAccount(accountId) {
    return verifyAccount(accountId)
  },
}
