export type AccountDriver = "jmap" | "mock";

export interface AppSettings {
  defaultAccountId: string | null;
}

export interface SecretStatus {
  storage: "env" | "os";
  configured: boolean;
  label: string | null;
}

export type KnownMailboxRole =
  | "inbox"
  | "archive"
  | "drafts"
  | "sent"
  | "junk"
  | "trash";

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

export interface AccountTransportInput {
  baseUrl: string;
  username: string;
}

export interface SecretInstructionInput {
  mode: "keep" | "replace" | "clear";
  password?: string;
}

export interface CreateAccountInput {
  id: string;
  name: string;
  driver: AccountDriver;
  enabled: boolean;
  transport: AccountTransportInput;
  secret: SecretInstructionInput;
}

export interface UpdateAccountInput {
  name?: string;
  driver?: AccountDriver;
  enabled?: boolean;
  transport?: Partial<AccountTransportInput>;
  secret?: SecretInstructionInput;
}

export interface VerificationResponse {
  ok: boolean;
  identityEmail: string | null;
  pushSupported: boolean;
}

export interface OkResponse {
  ok: boolean;
}

export interface Mailbox {
  id: string;
  name: string;
  role: KnownMailboxRole | null;
  unreadEmails: number;
  totalEmails: number;
}

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

export interface RawMessageRef {
  path: string;
  sha256: string;
  size: number;
  mimeType: string;
  fetchedAt: string;
}

export interface MessageDetail extends MessageSummary {
  bodyHtml: string | null;
  bodyText: string | null;
  rawMessage: RawMessageRef | null;
}

export interface SourceMessageRef {
  sourceId: string;
  messageId: string;
}

export type SmartMailboxKind = "default" | "user";

export type SmartMailboxGroupOperator = "all" | "any";

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

export type SmartMailboxOperator =
  | "equals"
  | "in"
  | "contains"
  | "before"
  | "after"
  | "onOrBefore"
  | "onOrAfter";

export type SmartMailboxValue = string | string[] | boolean;

export interface SmartMailboxGroup {
  operator: SmartMailboxGroupOperator;
  negated: boolean;
  nodes: SmartMailboxRuleNode[];
}

export interface SmartMailboxCondition {
  type: "condition";
  field: SmartMailboxField;
  operator: SmartMailboxOperator;
  negated: boolean;
  value: SmartMailboxValue;
}

export interface SmartMailboxRuleGroup {
  type: "group";
  operator: SmartMailboxGroupOperator;
  negated: boolean;
  nodes: SmartMailboxRuleNode[];
}

export type SmartMailboxRuleNode = SmartMailboxRuleGroup | SmartMailboxCondition;

export interface SmartMailboxRule {
  root: SmartMailboxGroup;
}

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

export interface ConversationPage {
  items: ConversationSummary[];
  nextCursor: string | null;
}

export interface ConversationView {
  id: string;
  subject: string | null;
  messages: MessageSummary[];
}

export interface SidebarSmartMailbox {
  id: string;
  name: string;
  unreadMessages: number;
  totalMessages: number;
}

export interface SidebarSource {
  id: string;
  name: string;
  mailboxes: Mailbox[];
}

export interface SidebarResponse {
  smartMailboxes: SidebarSmartMailbox[];
  sources: SidebarSource[];
}

export interface DomainEvent {
  seq: number;
  accountId: string;
  topic: string;
  occurredAt: string;
  mailboxId: string | null;
  messageId: string | null;
  payload: Record<string, unknown>;
}

export interface MessageCommandResult {
  detail: MessageDetail | null;
  events: DomainEvent[];
}

export type MessageCommand =
  | { kind: "setKeywords"; add: string[]; remove: string[] }
  | { kind: "addToMailbox"; mailboxId: string }
  | { kind: "removeFromMailbox"; mailboxId: string }
  | { kind: "replaceMailboxes"; mailboxIds: string[] }
  | { kind: "destroy" };

export interface CreateSmartMailboxInput {
  name: string;
  position?: number;
  rule: SmartMailboxRule;
}

export interface UpdateSmartMailboxInput {
  name?: string;
  position?: number;
  rule?: SmartMailboxRule;
}
