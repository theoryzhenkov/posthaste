import type {
  Email,
  EmailAction,
  EmailBody,
  IdentityResponse,
  Mailbox,
  PreviewResponse,
  ReplyDataResponse,
  SendEmailRequest,
} from "./types";
import { ApiError } from "./errors";

const BASE_URL = "http://localhost:3001/api";

async function request<T>(path: string): Promise<T> {
  const response = await fetch(`${BASE_URL}${path}`);
  if (!response.ok) {
    throw new ApiError(response.status, response.statusText);
  }
  return response.json() as Promise<T>;
}

async function postRequest<T>(path: string, body: unknown): Promise<T> {
  const response = await fetch(`${BASE_URL}${path}`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
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

export async function performEmailAction(
  emailId: string,
  action: EmailAction,
): Promise<Email> {
  return postRequest<Email>(`/emails/${emailId}/actions`, action);
}

export async function sendEmail(req: SendEmailRequest): Promise<void> {
  const response = await fetch(`${BASE_URL}/compose/send`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(req),
  });
  if (!response.ok) {
    throw new ApiError(response.status, response.statusText);
  }
}

export async function previewMarkdown(body: string): Promise<PreviewResponse> {
  return postRequest<PreviewResponse>("/compose/preview", { body });
}

export async function fetchIdentity(): Promise<IdentityResponse> {
  return request<IdentityResponse>("/identity");
}

export async function fetchReplyData(
  emailId: string,
): Promise<ReplyDataResponse> {
  return request<ReplyDataResponse>(`/emails/${emailId}/reply-data`);
}
