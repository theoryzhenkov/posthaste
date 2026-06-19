import {
  queueReject,
  queueResolve,
  type FakeQueues,
  type FakeRuntimeAdapter,
} from './fakeAdapterSupport'

type QueueControls = Pick<
  FakeRuntimeAdapter,
  | 'queueAccount'
  | 'queueAccountError'
  | 'queueAccountOk'
  | 'queueAccountOkError'
  | 'queueAccounts'
  | 'queueAccountsError'
  | 'queueConversation'
  | 'queueConversationError'
  | 'queueMailboxes'
  | 'queueMailboxesError'
  | 'queueMessage'
  | 'queueMessageError'
  | 'queueMessageCommandResult'
  | 'queueMessageCommandError'
  | 'queueMessagePage'
  | 'queueMessagePageError'
  | 'queueOpenMessageListView'
  | 'queueOpenMessageListViewError'
  | 'queueOAuthStartResponse'
  | 'queueOAuthStartError'
  | 'queueResourceBlob'
  | 'queueResourceError'
  | 'queueReadResponse'
  | 'queueReadError'
  | 'queueSmartMailboxes'
  | 'queueSmartMailboxesError'
  | 'queueSyncResult'
  | 'queueSyncError'
  | 'queueVerificationResponse'
  | 'queueVerificationError'
>

export function createFakeQueueControls(queues: FakeQueues): QueueControls {
  return {
    queueAccount: (account) => queueResolve(queues.accountResults, account),
    queueAccountError: (error) => queueReject(queues.accountResults, error),
    queueAccountOk: (result) => queueResolve(queues.accountOkResults, result),
    queueAccountOkError: (error) => queueReject(queues.accountOkResults, error),
    queueAccounts: (accounts) => queueResolve(queues.accounts, accounts),
    queueAccountsError: (error) => queueReject(queues.accounts, error),
    queueConversation: (conversation) =>
      queueResolve(queues.conversations, conversation),
    queueConversationError: (error) => queueReject(queues.conversations, error),
    queueMailboxes: (mailboxes) => queueResolve(queues.mailboxes, mailboxes),
    queueMailboxesError: (error) => queueReject(queues.mailboxes, error),
    queueMessage: (message) => queueResolve(queues.messages, message),
    queueMessageError: (error) => queueReject(queues.messages, error),
    queueMessageCommandResult: (result) =>
      queueResolve(queues.messageCommands, result),
    queueMessageCommandError: (error) =>
      queueReject(queues.messageCommands, error),
    queueMessagePage: (page) => queueResolve(queues.messagePages, page),
    queueMessagePageError: (error) => queueReject(queues.messagePages, error),
    queueOpenMessageListView: (result) =>
      queueResolve(queues.openMessageListViews, result),
    queueOpenMessageListViewError: (error) =>
      queueReject(queues.openMessageListViews, error),
    queueOAuthStartResponse: (response) =>
      queueResolve(queues.oauthStartResponses, response),
    queueOAuthStartError: (error) =>
      queueReject(queues.oauthStartResponses, error),
    queueReadResponse: (response) => queueResolve(queues.reads, response),
    queueReadError: (error) => queueReject(queues.reads, error),
    queueResourceBlob: (blob) => queueResolve(queues.resources, blob),
    queueResourceError: (error) => queueReject(queues.resources, error),
    queueSmartMailboxes: (mailboxes) =>
      queueResolve(queues.smartMailboxes, mailboxes),
    queueSmartMailboxesError: (error) =>
      queueReject(queues.smartMailboxes, error),
    queueSyncResult: (result) => queueResolve(queues.syncs, result),
    queueSyncError: (error) => queueReject(queues.syncs, error),
    queueVerificationResponse: (response) =>
      queueResolve(queues.verificationResponses, response),
    queueVerificationError: (error) =>
      queueReject(queues.verificationResponses, error),
  }
}
