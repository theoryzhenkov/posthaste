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
