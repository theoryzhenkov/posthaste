import type {
  RuntimeFrameHandlers,
  RuntimeMutationReceipt,
  RuntimeRunMutationRequest,
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

function defaultRuntimeMutationReceipt(
  request: RuntimeRunMutationRequest,
): RuntimeMutationReceipt {
  return {
    runtimeMutationId: 'mutation-1',
    clientMutationId: request.clientMutationId,
    name: request.name,
    state: 'confirmed',
    error: null,
    output: defaultMessageCommandResult,
  }
}

/** Fake runtime adapter for renderer tests. Does not start or contact a backend. */
export function createFakeRuntimeAdapter(
  input?: FakeRuntimeAdapterOptions,
): FakeRuntimeAdapter {
  const calls = createFakeCallRecords()
  const queues = createFakeQueues()
  const runtimeFrameHandlers = new Set<RuntimeFrameHandlers>()
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
    emitRuntimeFrame(frame) {
      for (const handlers of runtimeFrameHandlers) handlers.onFrame(frame)
    },
    emitRuntimeFrameStreamClosed(error) {
      // Simulate a hard transport close (e.g. an intermittent WKWebView drop):
      // notify the current frame subscribers and detach them, as a real closed
      // stream would. The link client should then reconnect on its own.
      const closing = [...runtimeFrameHandlers]
      runtimeFrameHandlers.clear()
      for (const handlers of closing) handlers.onClosed?.(error)
    },
    ...createFakeQueueControls(queues),
    reset() {
      accountCalls = 0
      smartMailboxCalls = 0
      runtimeFrameHandlers.clear()
      resetFakeCallRecords(calls)
      resetFakeQueues(queues)
    },
    openRuntimeLink(request) {
      calls.runtimeLinkCalls.push({ ...request })
      return resolveQueued(
        queues.runtimeLinks,
        input?.defaultRuntimeLinkConnection ?? { linkId: 'link-1' },
      )
    },
    closeRuntimeLink(request) {
      calls.runtimeLinkCloseCalls.push({ ...request })
      return resolveQueued(
        queues.accountOkResults,
        input?.defaultAccountOk ?? { ok: true },
      )
    },
    openRuntimeLinkMessageListView(request) {
      calls.runtimeLinkViewOpenCalls.push({
        linkId: request.linkId,
        view: { ...request.view },
        sourceId: request.sourceId,
      })
      return resolveQueuedOptional(
        queues.runtimeLinkMessageListViews,
        input?.defaultRuntimeLinkMessageListView,
        'runtime link message list view result',
      )
    },
    openRuntimeLinkView(request) {
      calls.runtimeLinkObjectViewOpenCalls.push({
        linkId: request.linkId,
        descriptor: request.descriptor,
        sourceId: request.sourceId,
      })
      return resolveQueuedOptional(
        queues.runtimeLinkViews,
        input?.defaultRuntimeLinkView,
        'runtime link view result',
      )
    },
    extendRuntimeLinkView(request) {
      calls.runtimeLinkViewExtendCalls.push({ ...request })
      return resolveQueuedOptional(
        queues.runtimeLinkViewExtends,
        input?.defaultRuntimeLinkViewExtend,
        'runtime link view extend result',
      )
    },
    closeRuntimeLinkView(request) {
      calls.runtimeLinkViewCloseCalls.push({ ...request })
      return resolveQueued(
        queues.accountOkResults,
        input?.defaultAccountOk ?? { ok: true },
      )
    },
    runRuntimeMutation(request) {
      calls.runtimeMutationCalls.push({ request: { ...request } })
      return resolveQueued(
        queues.runtimeMutations,
        input?.defaultRuntimeMutationReceipt ??
          defaultRuntimeMutationReceipt(request),
      )
    },
    subscribeRuntimeFrames(request, handlers) {
      calls.runtimeFrameSubscriptionCalls.push({ request })
      runtimeFrameHandlers.add(handlers)
      return () => runtimeFrameHandlers.delete(handlers)
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
    fetchDraftContent() {
      return unsupported('draft content')
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
    createMailbox() {
      return unsupported('mailbox create result')
    },
    deleteMailbox() {
      return unsupported('mailbox delete result')
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
    fetchRules() {
      return Promise.resolve([])
    },
    createRule() {
      return unsupported('rule create result')
    },
    updateRule() {
      return unsupported('rule update result')
    },
    deleteRule() {
      return Promise.resolve()
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
    saveDraft() {
      return unsupported('save draft result')
    },
    deleteDraft() {
      return unsupported('delete draft result')
    },
    listPendingOperations() {
      return Promise.resolve([])
    },
    discardOperation() {
      return Promise.resolve()
    },
    retryOperation() {
      return Promise.resolve()
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
