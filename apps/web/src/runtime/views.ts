import type {
  AccountOverview,
  AppSettings,
  CachedSenderAddress,
  ConversationPage,
  ConversationView,
  DraftContent,
  Identity,
  Mailbox,
  MessageDetail,
  MessagePage,
  ReadRequest,
  ReadResponse,
  ReplyContext,
  SmartMailbox,
  SmartMailboxSummary,
} from '../api/types'
import { getRuntimeAdapter } from './adapter'
import type {
  RuntimeConversationPageRequest,
  RuntimeMessagePageRequest,
  RuntimeReplyContextRequest,
} from './types'

export const runtimeViews = {
  accounts: {
    list(): Promise<AccountOverview[]> {
      return getRuntimeAdapter().fetchAccounts()
    },
    detail(accountId: string): Promise<AccountOverview> {
      return getRuntimeAdapter().fetchAccount(accountId)
    },
  },
  compose: {
    conversationPage(
      request?: RuntimeConversationPageRequest,
    ): Promise<ConversationPage> {
      return getRuntimeAdapter().fetchConversationPage(request)
    },
    identity(sourceId: string): Promise<Identity> {
      return getRuntimeAdapter().fetchIdentity(sourceId)
    },
    replyContext(request: RuntimeReplyContextRequest): Promise<ReplyContext> {
      return getRuntimeAdapter().fetchReplyContext(request)
    },
    draftContent(request: RuntimeReplyContextRequest): Promise<DraftContent> {
      return getRuntimeAdapter().fetchDraftContent(request)
    },
    senderAddresses(): Promise<CachedSenderAddress[]> {
      return getRuntimeAdapter().fetchSenderAddresses()
    },
  },
  mail: {
    conversation(conversationId: string): Promise<ConversationView> {
      return getRuntimeAdapter().fetchConversation(conversationId)
    },
    mailboxes(accountId: string): Promise<Mailbox[]> {
      return getRuntimeAdapter().fetchMailboxes(accountId)
    },
    message(messageId: string, sourceId: string): Promise<MessageDetail> {
      return getRuntimeAdapter().fetchMessage(messageId, sourceId)
    },
    messagePage(request: RuntimeMessagePageRequest): Promise<MessagePage> {
      return getRuntimeAdapter().fetchMessagePage(request)
    },
    read(request: ReadRequest): Promise<ReadResponse> {
      return getRuntimeAdapter().read(request)
    },
  },
  oauth: {
    redirectUri(): string {
      return getRuntimeAdapter().fetchOAuthRedirectUri()
    },
  },
  settings: {
    current(): Promise<AppSettings> {
      return getRuntimeAdapter().fetchSettings()
    },
  },
  smartMailboxes: {
    detail(smartMailboxId: string): Promise<SmartMailbox> {
      return getRuntimeAdapter().fetchSmartMailbox(smartMailboxId)
    },
    list(): Promise<SmartMailboxSummary[]> {
      return getRuntimeAdapter().fetchSmartMailboxes()
    },
  },
}
