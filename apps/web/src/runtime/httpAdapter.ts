import {
  EventStreamContentType,
  fetchEventSource,
} from '@microsoft/fetch-event-source'

import {
  authHeaders,
  buildAccountLogoUrl,
  buildEventsUrl,
  buildMessageAttachmentUrl,
  buildRuntimeSessionStreamUrl,
  buildViewStreamUrl,
  buildOAuthRedirectUri,
  closeRuntimeSession,
  closeRuntimeSessionView,
  createAccount,
  createSmartMailbox,
  deleteAccount,
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
  fetchSearchMessages,
  fetchSenderAddresses,
  fetchSettings,
  fetchSmartMailbox,
  fetchSmartMailboxMessages,
  fetchSmartMailboxes,
  fetchSourceMessages,
  openRuntimeSession,
  openRuntimeSessionView,
  openView,
  patchMailbox,
  patchSettings,
  performMessageCommand,
  previewAutomationRule,
  read,
  resetDefaultSmartMailboxes,
  runRuntimeMutation,
  saveDraft,
  deleteDraft,
  listPendingOperations,
  sendMessage,
  startProviderOAuth,
  triggerSync,
  updateAccount,
  updateSmartMailbox,
  uploadAccountLogo,
  verifyAccount,
} from '../api/client'

import type { DomainEvent, KnownMailboxRole, Mailbox } from '../api/types'

import type {
  RuntimeAdapter,
  RuntimeEventHandlers,
  RuntimeFrame,
  RuntimeFrameHandlers,
  RuntimeMailListViewState,
  RuntimeMailQueryRequest,
  RuntimeMessagePageRequest,
  RuntimeResourceDescriptor,
  RuntimeViewFrame,
  RuntimeViewFrameHandlers,
  RuntimeViewSnapshot,
} from './types'

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

function sourceScope(request: RuntimeMessagePageRequest): string | null {
  return request.scope.kind === 'source-mailbox' ? request.scope.sourceId : null
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

class FatalStreamError extends Error {}

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
  }
}

function handleMalformedFrame(
  handlers:
    | RuntimeEventHandlers
    | RuntimeFrameHandlers
    | RuntimeViewFrameHandlers,
  raw: string,
  error: unknown,
): void {
  handlers.onMalformedFrame?.({ raw, error })
}

export const httpRuntimeAdapter: RuntimeAdapter = {
  openRuntimeSession(request) {
    return openRuntimeSession({ sourceId: request.sourceId })
  },
  closeRuntimeSession(request) {
    return closeRuntimeSession(request.sessionId, {
      sourceId: request.sourceId,
    })
  },
  async openRuntimeSessionMessageListView(request) {
    const descriptor = {
      family: 'mailList',
      payload: mailQueryRequest(request.view),
    }
    return openRuntimeSessionView<
      RuntimeViewSnapshot<RuntimeMailListViewState>
    >(request.sessionId, { descriptor }, { sourceId: request.sourceId })
  },
  openRuntimeSessionView(request) {
    return openRuntimeSessionView(
      request.sessionId,
      { descriptor: request.descriptor },
      { sourceId: request.sourceId },
    )
  },
  closeRuntimeSessionView(request) {
    return closeRuntimeSessionView(request.sessionId, request.viewId, {
      sourceId: request.sourceId,
    })
  },
  runRuntimeMutation(request) {
    if (!request.sessionId) {
      return Promise.reject(new Error('runtime mutation requires a session id'))
    }
    return runRuntimeMutation(
      request.sessionId,
      {
        sessionId: request.sessionId,
        name: request.name,
        args: request.args,
        clientMutationId: request.clientMutationId,
        context: request.context,
      },
      { sourceId: request.sourceId },
    )
  },
  subscribeRuntimeFrames(request, handlers) {
    const controller = new AbortController()
    void fetchEventSource(
      buildRuntimeSessionStreamUrl({
        sessionId: request.sessionId,
        afterSeq: request.afterSeq,
        sourceId: request.sourceId,
      }),
      {
        headers: authHeaders(),
        signal: controller.signal,
        openWhenHidden: true,
        async onopen(response) {
          const contentType = response.headers.get('content-type') ?? ''
          if (response.ok && contentType.startsWith(EventStreamContentType)) {
            return
          }
          if (response.status >= 400 && response.status < 500) {
            throw new FatalStreamError(
              `runtime stream rejected with ${response.status}`,
            )
          }
          throw new Error(`runtime stream returned ${response.status}`)
        },
        onmessage(event) {
          if (!event.data) {
            return
          }
          let payload: RuntimeFrame<RuntimeMailListViewState>
          try {
            payload = JSON.parse(
              event.data,
            ) as RuntimeFrame<RuntimeMailListViewState>
          } catch (error) {
            handleMalformedFrame(handlers, event.data, error)
            return
          }
          handlers.onFrame(payload)
        },
        onerror(error) {
          if (error instanceof FatalStreamError) {
            handlers.onPermanentError?.(error)
            throw error
          }
          handlers.onTransientError?.(error)
        },
      },
    ).catch((error) => {
      if (controller.signal.aborted || error instanceof FatalStreamError) {
        return
      }
      handlers.onClosed?.(error)
    })
    return () => controller.abort()
  },
  async openMessageListView(request) {
    const descriptor = {
      family: 'mailList',
      payload: mailQueryRequest(request),
    }
    return openView<RuntimeViewSnapshot<RuntimeMailListViewState>>(
      { descriptor },
      { sourceId: sourceScope(request) },
    )
  },
  subscribeView(request, handlers) {
    const controller = new AbortController()
    void fetchEventSource(
      buildViewStreamUrl({
        viewId: request.viewId,
        afterRevision: request.afterRevision,
        sourceId: request.sourceId,
      }),
      {
        headers: authHeaders(),
        signal: controller.signal,
        openWhenHidden: true,
        async onopen(response) {
          const contentType = response.headers.get('content-type') ?? ''
          if (response.ok && contentType.startsWith(EventStreamContentType)) {
            return
          }
          if (response.status >= 400 && response.status < 500) {
            throw new FatalStreamError(
              `view stream rejected with ${response.status}`,
            )
          }
          throw new Error(`view stream returned ${response.status}`)
        },
        onmessage(event) {
          if (!event.data) {
            return
          }
          let payload: RuntimeViewFrame<RuntimeMailListViewState>
          try {
            payload = JSON.parse(
              event.data,
            ) as RuntimeViewFrame<RuntimeMailListViewState>
          } catch (error) {
            handleMalformedFrame(handlers, event.data, error)
            return
          }
          handlers.onFrame(payload)
        },
        onerror(error) {
          if (error instanceof FatalStreamError) {
            handlers.onPermanentError?.(error)
            throw error
          }
          handlers.onTransientError?.(error)
        },
      },
    ).catch((error) => {
      if (controller.signal.aborted || error instanceof FatalStreamError) {
        return
      }
      handlers.onClosed?.(error)
    })
    return () => controller.abort()
  },
  subscribeEvents(request, handlers) {
    const controller = new AbortController()
    void fetchEventSource(buildEventsUrl({ afterSeq: request.afterSeq }), {
      headers: authHeaders(),
      signal: controller.signal,
      openWhenHidden: true,
      async onopen(response) {
        const contentType = response.headers.get('content-type') ?? ''
        if (response.ok && contentType.startsWith(EventStreamContentType)) {
          return
        }
        if (response.status >= 400 && response.status < 500) {
          throw new FatalStreamError(
            `event stream rejected with ${response.status}`,
          )
        }
        throw new Error(`event stream returned ${response.status}`)
      },
      onmessage(event) {
        if (!event.data) {
          return
        }
        let payload: DomainEvent
        try {
          payload = JSON.parse(event.data) as DomainEvent
        } catch (error) {
          handleMalformedFrame(handlers, event.data, error)
          return
        }
        handlers.onEvent(payload)
      },
      onerror(error) {
        if (error instanceof FatalStreamError) {
          handlers.onPermanentError?.(error)
          throw error
        }
        handlers.onTransientError?.(error)
      },
    }).catch((error) => {
      if (controller.signal.aborted || error instanceof FatalStreamError) {
        return
      }
      handlers.onClosed?.(error)
    })
    return () => controller.abort()
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
  patchSettings(input) {
    return patchSettings(input)
  },
  previewAutomationRule(input) {
    return previewAutomationRule(input)
  },
  read(request) {
    return read(request)
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
