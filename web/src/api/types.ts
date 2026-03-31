export const DEFAULT_ACCOUNT_ID = "primary";

export interface Mailbox {
  id: string;
  name: string;
  role: string | null;
  unreadEmails: number;
  totalEmails: number;
}

export interface MessageSummary {
  id: string;
  threadId: string;
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

export interface ThreadView {
  id: string;
  messages: MessageSummary[];
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

export type MessageCommand =
  | { kind: "setKeywords"; add: string[]; remove: string[] }
  | { kind: "addToMailbox"; mailboxId: string }
  | { kind: "removeFromMailbox"; mailboxId: string }
  | { kind: "replaceMailboxes"; mailboxIds: string[] }
  | { kind: "destroy" };
