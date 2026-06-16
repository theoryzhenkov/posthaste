import {
  EventStreamContentType,
  fetchEventSource,
} from '@microsoft/fetch-event-source'

import {
  authHeaders,
  buildAccountLogoUrl,
  buildEventsUrl,
  buildMessageAttachmentUrl,
  createSmartMailbox,
  deleteSmartMailbox,
  fetchAccounts,
  fetchConversation,
  fetchConversations,
  fetchIdentity,
  fetchMailboxes,
  fetchMessage,
  fetchReplyContext,
  fetchSearchMessages,
  fetchSenderAddresses,
  fetchSettings,
  fetchSmartMailbox,
  fetchSmartMailboxMessages,
  fetchSmartMailboxes,
  fetchSourceMessages,
  patchMailbox,
  patchSettings,
  performMessageCommand,
  previewAutomationRule,
  read,
  resetDefaultSmartMailboxes,
  sendMessage,
  triggerSync,
  updateSmartMailbox,
} from '../api/client'

import type { DomainEvent } from '../api/types'

import type {
  RuntimeAdapter,
  RuntimeEventHandlers,
  RuntimeMessagePageRequest,
  RuntimeResourceDescriptor,
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

class FatalStreamError extends Error {}

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
  handlers: RuntimeEventHandlers,
  raw: string,
  error: unknown,
): void {
  handlers.onMalformedFrame?.({ raw, error })
}

export const httpRuntimeAdapter: RuntimeAdapter = {
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
  createSmartMailbox(input) {
    return createSmartMailbox(input)
  },
  deleteSmartMailbox(smartMailboxId) {
    return deleteSmartMailbox(smartMailboxId)
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
  fetchReplyContext({ sourceId, messageId }) {
    return fetchReplyContext(sourceId, messageId)
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
  resetDefaultSmartMailboxes() {
    return resetDefaultSmartMailboxes()
  },
  sendMessage({ sourceId, input }) {
    return sendMessage(sourceId, input)
  },
  triggerSync(request) {
    return triggerSync(request)
  },
  updateSmartMailbox(smartMailboxId, input) {
    return updateSmartMailbox(smartMailboxId, input)
  },
}
