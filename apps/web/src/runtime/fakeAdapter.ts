import type {
  RuntimeEventHandlers,
  RuntimeFrameHandlers,
  RuntimeViewFrameHandlers,
} from './types'
import { createFakeQueueControls } from './fakeAdapterQueues'
import {
  createFakeCallRecords,
  createFakeQueues,
  defaultAccounts,
  defaultMailboxes,
  defaultMessageCommandResult,
  defaultMessagePage,
  defaultReadResponse,
  defaultSmartMailboxes,
  resetFakeCallRecords,
  resetFakeQueues,
  resolveQueued,
  resolveQueuedOptional,
  unsupported,
  type FakeRuntimeAdapter,
  type FakeRuntimeAdapterOptions,
} from './fakeAdapterSupport'

export type { FakeRuntimeAdapter } from './fakeAdapterSupport'

/** Fake runtime adapter for renderer tests. Does not start or contact a backend. */
export function createFakeRuntimeAdapter(
  input?: FakeRuntimeAdapterOptions,
): FakeRuntimeAdapter {
  const calls = createFakeCallRecords()
  const queues = createFakeQueues()
  const eventHandlers = new Set<RuntimeEventHandlers>()
  const runtimeFrameHandlers = new Set<RuntimeFrameHandlers>()
  const viewHandlers = new Set<RuntimeViewFrameHandlers>()
  let accountCalls = 0
  let smartMailboxCalls = 0

  return {
    get accountCalls() {
      return accountCalls
    },
    ...calls,
    get smartMailboxCalls() {
      return smartMailboxCalls
    },
    emitDomainEvent(event) {
      for (const handlers of eventHandlers) handlers.onEvent(event)
    },
    emitRuntimeFrame(frame) {
      for (const handlers of runtimeFrameHandlers) handlers.onFrame(frame)
    },
    emitViewFrame(frame) {
      for (const handlers of viewHandlers) handlers.onFrame(frame)
    },
    ...createFakeQueueControls(queues),
    reset() {
      accountCalls = 0
      smartMailboxCalls = 0
      eventHandlers.clear()
      runtimeFrameHandlers.clear()
      viewHandlers.clear()
      resetFakeCallRecords(calls)
      resetFakeQueues(queues)
    },
    subscribeEvents(request, handlers) {
      calls.eventSubscriptionCalls.push({ request })
      eventHandlers.add(handlers)
      return () => eventHandlers.delete(handlers)
    },
    openRuntimeSession(request) {
      calls.runtimeSessionCalls.push({ ...request })
      return resolveQueued(
        queues.runtimeSessions,
        input?.defaultRuntimeSession ?? { sessionId: 'session-1' },
      )
    },
    closeRuntimeSession(request) {
      calls.runtimeSessionCloseCalls.push({ ...request })
      return resolveQueued(
        queues.accountOkResults,
        input?.defaultAccountOk ?? { ok: true },
      )
    },
    openRuntimeSessionMessageListView(request) {
      calls.runtimeSessionViewOpenCalls.push({
        sessionId: request.sessionId,
        view: { ...request.view },
      })
      return resolveQueuedOptional(
        queues.runtimeSessionMessageListViews,
        input?.defaultRuntimeSessionMessageListView,
        'runtime session message list view result',
      )
    },
    subscribeRuntimeFrames(request, handlers) {
      calls.runtimeFrameSubscriptionCalls.push({ request })
      runtimeFrameHandlers.add(handlers)
      return () => runtimeFrameHandlers.delete(handlers)
    },
    openMessageListView(request) {
      calls.viewOpenCalls.push({ ...request })
      return resolveQueuedOptional(
        queues.openMessageListViews,
        input?.defaultOpenMessageListView,
        'open message list view result',
      )
    },
    subscribeView(request, handlers) {
      calls.viewSubscriptionCalls.push({ request })
      viewHandlers.add(handlers)
      return () => viewHandlers.delete(handlers)
    },
    createAccount(accountInput) {
      calls.accountCreateCalls.push(accountInput)
      return resolveQueuedOptional(
        queues.accountResults,
        input?.defaultAccount,
        'account result',
      )
    },
    createSmartMailbox() {
      return unsupported('smart mailbox result')
    },
    deleteAccount(accountId) {
      calls.accountCommandCalls.push({ kind: 'delete', accountId })
      return resolveQueued(
        queues.accountOkResults,
        input?.defaultAccountOk ?? { ok: true },
      )
    },
    deleteSmartMailbox() {
      return unsupported('smart mailbox delete result')
    },
    disableAccount(accountId) {
      calls.accountCommandCalls.push({ kind: 'disable', accountId })
      return resolveQueued(
        queues.accountOkResults,
        input?.defaultAccountOk ?? { ok: true },
      )
    },
    enableAccount(accountId) {
      calls.accountCommandCalls.push({ kind: 'enable', accountId })
      return resolveQueued(
        queues.accountOkResults,
        input?.defaultAccountOk ?? { ok: true },
      )
    },
    fetchAccount(accountId) {
      calls.accountDetailCalls.push(accountId)
      return resolveQueuedOptional(
        queues.accountResults,
        input?.defaultAccount,
        'account result',
      )
    },
    fetchAccounts() {
      accountCalls += 1
      return resolveQueued(
        queues.accounts,
        input?.defaultAccounts ?? defaultAccounts,
      )
    },
    fetchConversation(conversationId) {
      calls.conversationCalls.push(conversationId)
      return resolveQueuedOptional(
        queues.conversations,
        input?.defaultConversation,
        'conversation result',
      )
    },
    fetchConversationPage() {
      return Promise.resolve({ items: [], nextCursor: null })
    },
    fetchIdentity() {
      return unsupported('identity')
    },
    fetchMailboxes(accountId) {
      calls.mailboxCalls.push(accountId)
      return resolveQueued(
        queues.mailboxes,
        input?.defaultMailboxes ?? defaultMailboxes,
      )
    },
    fetchMessage(messageId, sourceId) {
      calls.messageCalls.push({ messageId, sourceId })
      return resolveQueuedOptional(
        queues.messages,
        input?.defaultMessage,
        'message result',
      )
    },
    fetchMessagePage(request) {
      calls.messagePageCalls.push({ ...request })
      return resolveQueued(
        queues.messagePages,
        input?.defaultMessagePage ?? defaultMessagePage,
      )
    },
    fetchOAuthRedirectUri() {
      return 'http://localhost:3001/v1/oauth/callback'
    },
    fetchReplyContext() {
      return unsupported('reply context')
    },
    fetchResourceBlob(descriptor) {
      calls.resourceCalls.push({ descriptor })
      return resolveQueuedOptional(
        queues.resources,
        undefined,
        'resource blob result',
      )
    },
    fetchSettings() {
      return unsupported('settings')
    },
    fetchSenderAddresses() {
      return Promise.resolve([])
    },
    fetchSmartMailbox() {
      return unsupported('smart mailbox result')
    },
    fetchSmartMailboxes() {
      smartMailboxCalls += 1
      return resolveQueued(
        queues.smartMailboxes,
        input?.defaultSmartMailboxes ?? defaultSmartMailboxes,
      )
    },
    patchMailbox() {
      return unsupported('mailbox patch result')
    },
    patchSettings() {
      return unsupported('settings')
    },
    previewAutomationRule() {
      return unsupported('automation preview result')
    },
    read(request) {
      calls.readCalls.push(request)
      return resolveQueued(
        queues.reads,
        input?.defaultReadResponse ?? defaultReadResponse,
      )
    },
    runMessageCommand(request) {
      calls.messageCommandCalls.push({ ...request })
      return resolveQueued(
        queues.messageCommands,
        input?.defaultMessageCommandResult ?? defaultMessageCommandResult,
      )
    },
    moveMessageToMailboxRole(request) {
      calls.messageRoleMoveCalls.push({ ...request })
      return resolveQueued(
        queues.messageCommands,
        input?.defaultMessageCommandResult ?? defaultMessageCommandResult,
      )
    },
    resetDefaultSmartMailboxes() {
      return Promise.resolve([])
    },
    sendMessage() {
      return unsupported('send message result')
    },
    startProviderOAuth(oauthInput) {
      calls.oauthStartCalls.push({
        provider: oauthInput.provider,
        clientId: oauthInput.clientId,
        redirectUri: oauthInput.redirectUri,
        hasClientSecret: Boolean(oauthInput.clientSecret),
      })
      return resolveQueuedOptional(
        queues.oauthStartResponses,
        input?.defaultOAuthStartResponse,
        'oauth start result',
      )
    },
    triggerSync(request) {
      calls.syncCalls.push(request)
      return resolveQueuedOptional(queues.syncs, undefined, 'sync result')
    },
    updateAccount(accountId, accountInput) {
      calls.accountUpdateCalls.push({ accountId, input: accountInput })
      return resolveQueuedOptional(
        queues.accountResults,
        input?.defaultAccount,
        'account result',
      )
    },
    updateSmartMailbox() {
      return unsupported('smart mailbox result')
    },
    uploadAccountLogo(accountId, file) {
      calls.accountLogoUploadCalls.push({ accountId, file })
      return resolveQueuedOptional(
        queues.accountResults,
        input?.defaultAccount,
        'account logo result',
      )
    },
    verifyAccount(accountId) {
      calls.accountVerificationCalls.push(accountId)
      return resolveQueuedOptional(
        queues.verificationResponses,
        input?.defaultVerificationResponse,
        'account verification result',
      )
    },
  }
}
