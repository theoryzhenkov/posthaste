export interface Mailbox {
  id: string;
  name: string;
  role: string | null;
  unreadEmails: number;
  totalEmails: number;
}

export interface Email {
  id: string;
  threadId: string;
  subject: string | null;
  fromName: string | null;
  fromEmail: string | null;
  preview: string | null;
  receivedAt: string; // ISO 8601
  hasAttachment: boolean;
  isRead: boolean;
  isFlagged: boolean;
  mailboxIds: string[];
  keywords: string[];
}

export interface EmailBody {
  emailId: string;
  html: string | null;
  text: string | null;
}

export type EmailAction =
  | { action: "markRead" }
  | { action: "markUnread" }
  | { action: "flag" }
  | { action: "unflag" }
  | { action: "archive" }
  | { action: "trash" }
  | { action: "delete" }
  | { action: "move"; mailboxId: string };
