/** @spec spec/L1-api#endpoint-table */
export type AccountDriver = "jmap" | "mock";

/** @spec spec/L1-api#endpoint-table */
export interface AppSettings {
  defaultAccountId: string | null;
}

/**
 * Redacted secret status returned by the API -- never contains the actual value.
 * @spec spec/L1-api#secret-management
 */
export interface SecretStatus {
  storage: "env" | "os";
  configured: boolean;
  label: string | null;
}

/** @spec spec/L1-api#endpoint-table */
export type KnownMailboxRole =
  | "inbox"
  | "archive"
  | "drafts"
  | "sent"
  | "junk"
  | "trash";

/**
 * Summary of a configured account, including transport and sync status.
 * @spec spec/L1-api#account-crud-lifecycle
 */
export interface AccountOverview {
  id: string;
  name: string;
  driver: AccountDriver;
  enabled: boolean;
  transport: {
    baseUrl: string | null;
    username: string | null;
    secret: SecretStatus;
  };
  createdAt: string;
  updatedAt: string;
  isDefault: boolean;
  status: "ready" | "syncing" | "degraded" | "authError" | "offline" | "disabled";
  push: "connected" | "reconnecting" | "unsupported" | "disabled";
  lastSyncAt: string | null;
  lastSyncError: string | null;
  lastSyncErrorCode: string | null;
}

/** @spec spec/L1-api#account-crud-lifecycle */
export interface AccountTransportInput {
  baseUrl: string;
  username: string;
}

/**
 * Tri-state secret write mode: keep existing, replace with new password, or clear.
 * @spec spec/L1-api#secret-management
 */
export interface SecretInstructionInput {
  mode: "keep" | "replace" | "clear";
  password?: string;
}

/** @spec spec/L1-api#account-crud-lifecycle */
export interface CreateAccountInput {
  id: string;
  name: string;
  driver: AccountDriver;
  enabled: boolean;
  transport: AccountTransportInput;
  secret: SecretInstructionInput;
}

/**
 * Sparse-merge update payload -- omitted fields are preserved.
 * @spec spec/L1-api#account-crud-lifecycle
 */
export interface UpdateAccountInput {
  name?: string;
  driver?: AccountDriver;
  enabled?: boolean;
  transport?: Partial<AccountTransportInput>;
  secret?: SecretInstructionInput;
}

/** @spec spec/L1-api#account-crud-lifecycle */
export interface VerificationResponse {
  ok: boolean;
  identityEmail: string | null;
  pushSupported: boolean;
}

/** @spec spec/L1-api#error-format */
export interface OkResponse {
  ok: boolean;
}

/** @spec spec/L1-api#endpoint-table */
export interface Mailbox {
  id: string;
  name: string;
  role: KnownMailboxRole | null;
  unreadEmails: number;
  totalEmails: number;
}

/**
 * Compact message metadata used in conversation rows and thread switchers.
 * @spec spec/L1-ui#messagelist
 */
export interface MessageSummary {
  id: string;
  sourceId: string;
  sourceName: string;
  sourceThreadId: string;
  conversationId: string;
  subject: string | null;
  fromName: string | null;
  fromEmail: string | null;
  preview: string | null;
  receivedAt: string;
  hasAttachment: boolean;
  isRead: boolean;
  isFlagged: boolean;
  mailboxIds: string[];
  keywords: string[];
}

/** Reference to a raw message file stored on the backend. */
export interface RawMessageRef {
  path: string;
  sha256: string;
  size: number;
  mimeType: string;
  fetchedAt: string;
}

/**
 * Full message detail including sanitized body HTML.
 * @spec spec/L1-api#message-body-sanitization
 */
export interface MessageDetail extends MessageSummary {
  bodyHtml: string | null;
  bodyText: string | null;
  rawMessage: RawMessageRef | null;
}

/** Pair that uniquely identifies a message within a source account. */
export interface SourceMessageRef {
  sourceId: string;
  messageId: string;
}

/** @spec spec/L1-search#smart-mailbox-data-model */
export type SmartMailboxKind = "default" | "user";

/** @spec spec/L1-search#smart-mailbox-data-model */
export type SmartMailboxGroupOperator = "all" | "any";

/** @spec spec/L1-search#smart-mailbox-data-model */
export type SmartMailboxField =
  | "sourceId"
  | "sourceName"
  | "mailboxId"
  | "mailboxRole"
  | "isRead"
  | "isFlagged"
  | "hasAttachment"
  | "keyword"
  | "fromName"
  | "fromEmail"
  | "subject"
  | "preview"
  | "receivedAt";

/** @spec spec/L1-search#smart-mailbox-data-model */
export type SmartMailboxOperator =
  | "equals"
  | "in"
  | "contains"
  | "before"
  | "after"
  | "onOrBefore"
  | "onOrAfter";

/** @spec spec/L1-search#smart-mailbox-data-model */
export type SmartMailboxValue = string | string[] | boolean;

/** @spec spec/L1-search#smart-mailbox-data-model */
export interface SmartMailboxGroup {
  operator: SmartMailboxGroupOperator;
  negated: boolean;
  nodes: SmartMailboxRuleNode[];
}

/** @spec spec/L1-search#smart-mailbox-data-model */
export interface SmartMailboxCondition {
  type: "condition";
  field: SmartMailboxField;
  operator: SmartMailboxOperator;
  negated: boolean;
  value: SmartMailboxValue;
}

/** @spec spec/L1-search#smart-mailbox-data-model */
export interface SmartMailboxRuleGroup {
  type: "group";
  operator: SmartMailboxGroupOperator;
  negated: boolean;
  nodes: SmartMailboxRuleNode[];
}

/** @spec spec/L1-search#smart-mailbox-data-model */
export type SmartMailboxRuleNode = SmartMailboxRuleGroup | SmartMailboxCondition;

/** @spec spec/L1-search#smart-mailbox-data-model */
export interface SmartMailboxRule {
  root: SmartMailboxGroup;
}

/** @spec spec/L1-api#smart-mailbox-crud */
export interface SmartMailbox {
  id: string;
  name: string;
  position: number;
  kind: SmartMailboxKind;
  defaultKey: string | null;
  parentId: string | null;
  rule: SmartMailboxRule;
  createdAt: string;
  updatedAt: string;
}

/** @spec spec/L1-api#smart-mailbox-crud */
export interface SmartMailboxSummary {
  id: string;
  name: string;
  position: number;
  kind: SmartMailboxKind;
  defaultKey: string | null;
  parentId: string | null;
  unreadMessages: number;
  totalMessages: number;
  createdAt: string;
  updatedAt: string;
}

/**
 * Locally derived conversation summary for middle-pane rows.
 * @spec spec/L1-sync#conversation-pagination
 */
export interface ConversationSummary {
  id: string;
  subject: string | null;
  preview: string | null;
  fromName: string | null;
  fromEmail: string | null;
  latestReceivedAt: string;
  unreadCount: number;
  messageCount: number;
  sourceIds: string[];
  sourceNames: string[];
  latestMessage: SourceMessageRef;
  latestSourceName: string;
  hasAttachment: boolean;
  isFlagged: boolean;
}

/**
 * Cursor-paginated conversation response.
 * @spec spec/L1-api#cursor-pagination
 */
export interface ConversationPage {
  items: ConversationSummary[];
  nextCursor: string | null;
}

/**
 * Full conversation view with all message summaries in the thread.
 * @spec spec/L1-ui#messagedetail-and-emailframe
 */
export interface ConversationView {
  id: string;
  subject: string | null;
  messages: MessageSummary[];
}

/** @spec spec/L1-ui#component-hierarchy */
export interface SidebarSmartMailbox {
  id: string;
  name: string;
  unreadMessages: number;
  totalMessages: number;
}

/** @spec spec/L1-ui#component-hierarchy */
export interface SidebarSource {
  id: string;
  name: string;
  mailboxes: Mailbox[];
}

/** @spec spec/L1-api#endpoint-table */
export interface SidebarResponse {
  smartMailboxes: SidebarSmartMailbox[];
  sources: SidebarSource[];
}

/**
 * Server-sent domain event from the event log.
 * @spec spec/L1-api#sse-event-stream
 */
export interface DomainEvent {
  seq: number;
  accountId: string;
  topic: string;
  occurredAt: string;
  mailboxId: string | null;
  messageId: string | null;
  payload: Record<string, unknown>;
}

/** @spec spec/L1-api#endpoint-table */
export interface MessageCommandResult {
  detail: MessageDetail | null;
  events: DomainEvent[];
}

/**
 * Discriminated union of all message commands the API accepts.
 * @spec spec/L1-api#endpoint-table
 */
export type MessageCommand =
  | { kind: "setKeywords"; add: string[]; remove: string[] }
  | { kind: "addToMailbox"; mailboxId: string }
  | { kind: "removeFromMailbox"; mailboxId: string }
  | { kind: "replaceMailboxes"; mailboxIds: string[] }
  | { kind: "destroy" };

/** @spec spec/L1-api#smart-mailbox-crud */
export interface CreateSmartMailboxInput {
  name: string;
  position?: number;
  rule: SmartMailboxRule;
}

/** @spec spec/L1-api#smart-mailbox-crud */
export interface UpdateSmartMailboxInput {
  name?: string;
  position?: number;
  rule?: SmartMailboxRule;
}
