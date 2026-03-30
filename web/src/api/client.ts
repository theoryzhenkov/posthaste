import type { Email, EmailBody, Mailbox } from "./types";
import { ApiError } from "./errors";

const BASE_URL = "http://localhost:3001/api";

async function request<T>(path: string): Promise<T> {
  const response = await fetch(`${BASE_URL}${path}`);
  if (!response.ok) {
    throw new ApiError(response.status, response.statusText);
  }
  return response.json() as Promise<T>;
}

export async function fetchMailboxes(): Promise<Mailbox[]> {
  return request<Mailbox[]>("/mailboxes");
}

export async function fetchEmails(mailboxId: string): Promise<Email[]> {
  return request<Email[]>(`/mailboxes/${mailboxId}/emails`);
}

export async function fetchEmail(emailId: string): Promise<Email> {
  return request<Email>(`/emails/${emailId}`);
}

export async function fetchThread(threadId: string): Promise<Email[]> {
  return request<Email[]>(`/threads/${threadId}`);
}

export async function fetchEmailBody(emailId: string): Promise<EmailBody> {
  return request<EmailBody>(`/emails/${emailId}/body`);
}
