/** @spec docs/L1-api#endpoint-table */
export type AccountDriver = 'jmap' | 'mock'

/** @spec docs/L1-api#endpoint-table */
export interface AppSettings {
  defaultAccountId: string | null
}

/**
 * Redacted secret status returned by the API -- never contains the actual value.
 * @spec docs/L1-api#secret-management
 */
export interface SecretStatus {
  storage: 'env' | 'os'
  configured: boolean
  label: string | null
}

/** @spec docs/L1-api#endpoint-table */
export type KnownMailboxRole =
  | 'inbox'
  | 'archive'
  | 'drafts'
  | 'sent'
  | 'junk'
  | 'trash'

/**
 * Summary of a configured account, including transport and sync status.
 * @spec docs/L1-api#account-crud-lifecycle
 */
export interface AccountOverview {
  id: string
  name: string
  fullName: string | null
  emailPatterns: string[]
  driver: AccountDriver
  enabled: boolean
  appearance: AccountAppearance
  transport: {
    baseUrl: string | null
    username: string | null
    secret: SecretStatus
  }
  createdAt: string
  updatedAt: string
  isDefault: boolean
  status:
    | 'ready'
    | 'syncing'
    | 'degraded'
    | 'authError'
    | 'offline'
    | 'disabled'
  push: 'connected' | 'reconnecting' | 'unsupported' | 'disabled'
  lastSyncAt: string | null
  lastSyncError: string | null
  lastSyncErrorCode: string | null
}

/** @spec docs/L1-api#account-crud-lifecycle */
export type AccountAppearance =
  | {
      kind: 'initials'
      initials: string
      colorHue: number
    }
  | {
      kind: 'image'
      imageId: string
      initials: string
      colorHue: number
    }

/** @spec docs/L1-api#account-crud-lifecycle */
export interface AccountTransportInput {
  baseUrl: string
  username: string
}

/**
 * Tri-state secret write mode: keep existing, replace with new password, or clear.
 * @spec docs/L1-api#secret-management
 */
export interface SecretInstructionInput {
  mode: 'keep' | 'replace' | 'clear'
  password?: string
}

/** @spec docs/L1-api#account-crud-lifecycle */
export interface CreateAccountInput {
  id?: string
  name: string
  fullName?: string | null
  emailPatterns: string[]
  driver?: AccountDriver
  enabled?: boolean
  appearance?: AccountAppearance
  transport: AccountTransportInput
  secret: SecretInstructionInput
}

/**
 * Sparse-merge update payload -- omitted fields are preserved.
 * @spec docs/L1-api#account-crud-lifecycle
 */
export interface UpdateAccountInput {
  name?: string
  fullName?: string | null
  emailPatterns?: string[]
  driver?: AccountDriver
  enabled?: boolean
  appearance?: AccountAppearance
  transport?: Partial<AccountTransportInput>
  secret?: SecretInstructionInput
}

/** @spec docs/L1-api#account-crud-lifecycle */
export interface VerificationResponse {
  ok: boolean
  identityEmail: string | null
  pushSupported: boolean
}

/** @spec docs/L1-api#compose */
export interface Identity {
  id: string
  name: string
  email: string
}

/** @spec docs/L1-api#compose */
export interface Recipient {
  name: string | null
  email: string
}

/** @spec docs/L1-api#compose */
export interface ReplyContext {
  to: Recipient[]
  cc: Recipient[]
  replySubject: string
  forwardSubject: string
  quotedBody: string | null
  inReplyTo: string | null
  references: string | null
}

/** @spec docs/L1-api#compose */
export interface SendMessageInput {
  to: Recipient[]
  cc: Recipient[]
  bcc: Recipient[]
  subject: string
  body: string
  inReplyTo: string | null
  references: string | null
}

/** @spec docs/L1-api#error-format */
export interface OkResponse {
  ok: boolean
}

/** @spec docs/L1-api#endpoint-table */
export interface Mailbox {
  id: string
  name: string
  role: string | null
  unreadEmails: number
  totalEmails: number
}

/** @spec docs/L1-api#endpoint-table */
export interface PatchMailboxInput {
  role: KnownMailboxRole | null
}

/**
 * Compact message metadata used in conversation rows and thread switchers.
 * @spec docs/L1-ui#messagelist
 */
export interface MessageSummary {
  id: string
  sourceId: string
  sourceName: string
  sourceThreadId: string
  conversationId: string
  subject: string | null
  fromName: string | null
  fromEmail: string | null
  preview: string | null
  receivedAt: string
  hasAttachment: boolean
  isRead: boolean
  isFlagged: boolean
  mailboxIds: string[]
  keywords: string[]
}

/** @spec docs/L1-api#cursor-pagination */
export type MessageSortField =
  | 'date'
  | 'from'
  | 'subject'
  | 'source'
  | 'flagged'
  | 'attachment'

/** @spec docs/L1-api#cursor-pagination */
export interface MessagePage {
  items: MessageSummary[]
  nextCursor: string | null
}

/**
 * Reference to a raw message file stored on the backend.
 * @spec docs/L1-sync#body-lazy
 */
export interface RawMessageRef {
  path: string
  sha256: string
  size: number
  mimeType: string
  fetchedAt: string
}

export interface MessageAttachment {
  id: string
  blobId: string
  partId: string | null
  filename: string | null
  mimeType: string
  size: number
  disposition: string | null
  cid: string | null
  isInline: boolean
}

/**
 * Full message detail including sanitized body HTML.
 * @spec docs/L1-api#message-body-sanitization
 */
export interface MessageDetail extends MessageSummary {
  bodyHtml: string | null
  bodyText: string | null
  rawMessage: RawMessageRef | null
  attachments: MessageAttachment[]
}

/**
 * Pair that uniquely identifies a message within a source account.
 * @spec docs/L1-api#endpoint-table
 */
export interface SourceMessageRef {
  sourceId: string
  messageId: string
}

/** @spec docs/L1-search#smart-mailbox-data-model */
export type SmartMailboxKind = 'default' | 'user'

/** @spec docs/L1-search#smart-mailbox-data-model */
export type SmartMailboxGroupOperator = 'all' | 'any'

/** @spec docs/L1-search#smart-mailbox-data-model */
export type SmartMailboxField =
  | 'sourceId'
  | 'sourceName'
  | 'messageId'
  | 'threadId'
  | 'mailboxId'
  | 'mailboxName'
  | 'mailboxRole'
  | 'isRead'
  | 'isFlagged'
  | 'hasAttachment'
  | 'keyword'
  | 'fromName'
  | 'fromEmail'
  | 'subject'
  | 'preview'
  | 'receivedAt'

/** @spec docs/L1-search#smart-mailbox-data-model */
export type SmartMailboxOperator =
  | 'equals'
  | 'in'
  | 'contains'
  | 'before'
  | 'after'
  | 'onOrBefore'
  | 'onOrAfter'

/** @spec docs/L1-search#smart-mailbox-data-model */
export type SmartMailboxValue = string | string[] | boolean

/** @spec docs/L1-search#smart-mailbox-data-model */
export interface SmartMailboxGroup {
  operator: SmartMailboxGroupOperator
  negated: boolean
  nodes: SmartMailboxRuleNode[]
}

/** @spec docs/L1-search#smart-mailbox-data-model */
export interface SmartMailboxCondition {
  type: 'condition'
  field: SmartMailboxField
  operator: SmartMailboxOperator
  negated: boolean
  value: SmartMailboxValue
}

/** @spec docs/L1-search#smart-mailbox-data-model */
export interface SmartMailboxRuleGroup {
  type: 'group'
  operator: SmartMailboxGroupOperator
  negated: boolean
  nodes: SmartMailboxRuleNode[]
}

/** @spec docs/L1-search#smart-mailbox-data-model */
export type SmartMailboxRuleNode = SmartMailboxRuleGroup | SmartMailboxCondition

/** @spec docs/L1-search#smart-mailbox-data-model */
export interface SmartMailboxRule {
  root: SmartMailboxGroup
}

/** @spec docs/L1-api#smart-mailbox-crud */
export interface SmartMailbox {
  id: string
  name: string
  position: number
  kind: SmartMailboxKind
  defaultKey: string | null
  parentId: string | null
  rule: SmartMailboxRule
  createdAt: string
  updatedAt: string
}

/** @spec docs/L1-api#smart-mailbox-crud */
export interface SmartMailboxSummary {
  id: string
  name: string
  position: number
  kind: SmartMailboxKind
  defaultKey: string | null
  parentId: string | null
  unreadMessages: number
  totalMessages: number
  createdAt: string
  updatedAt: string
}

/**
 * Locally derived conversation summary for middle-pane rows.
 * @spec docs/L1-sync#conversation-pagination
 */
export interface ConversationSummary {
  id: string
  subject: string | null
  preview: string | null
  fromName: string | null
  fromEmail: string | null
  latestReceivedAt: string
  unreadCount: number
  messageCount: number
  sourceIds: string[]
  sourceNames: string[]
  latestMessage: SourceMessageRef
  latestSourceName: string
  hasAttachment: boolean
  isFlagged: boolean
}

/**
 * Cursor-paginated conversation response.
 * @spec docs/L1-api#cursor-pagination
 */
export interface ConversationPage {
  items: ConversationSummary[]
  nextCursor: string | null
}

/**
 * Full conversation view with all message summaries in the thread.
 * @spec docs/L1-ui#messagedetail-and-emailframe
 */
export interface ConversationView {
  id: string
  subject: string | null
  messages: MessageSummary[]
}

/** @spec docs/L1-ui#component-hierarchy */
export interface SidebarSmartMailbox {
  id: string
  name: string
  unreadMessages: number
  totalMessages: number
}

/** @spec docs/L1-ui#component-hierarchy */
export interface SidebarSource {
  id: string
  name: string
  mailboxes: Mailbox[]
}

/** @spec docs/L1-api#endpoint-table */
export interface SidebarResponse {
  smartMailboxes: SidebarSmartMailbox[]
  sources: SidebarSource[]
}

/**
 * Server-sent domain event from the event log.
 * @spec docs/L1-api#sse-event-stream
 */
export interface DomainEvent {
  seq: number
  accountId: string
  topic: string
  occurredAt: string
  mailboxId: string | null
  messageId: string | null
  payload: Record<string, unknown>
}

/** @spec docs/L1-api#endpoint-table */
export interface MessageCommandResult {
  detail: MessageDetail | null
  events: DomainEvent[]
}

/**
 * Discriminated union of all message commands the API accepts.
 * @spec docs/L1-api#endpoint-table
 */
export type MessageCommand =
  | { kind: 'setKeywords'; add: string[]; remove: string[] }
  | { kind: 'addToMailbox'; mailboxId: string }
  | { kind: 'removeFromMailbox'; mailboxId: string }
  | { kind: 'replaceMailboxes'; mailboxIds: string[] }
  | { kind: 'destroy' }

/** @spec docs/L1-api#smart-mailbox-crud */
export interface CreateSmartMailboxInput {
  name: string
  position?: number
  rule: SmartMailboxRule
}

/** @spec docs/L1-api#smart-mailbox-crud */
export interface UpdateSmartMailboxInput {
  name?: string
  position?: number
  rule?: SmartMailboxRule
}
