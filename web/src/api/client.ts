import { ApiError } from "./errors";
import type {
  Mailbox,
  MessageCommand,
  MessageDetail,
  MessageSummary,
  ThreadView,
} from "./types";
import { DEFAULT_ACCOUNT_ID } from "./types";

const BASE_URL = "http://localhost:3001/v1";

async function parseError(response: Response): Promise<never> {
  let message = response.statusText;
  let code: string | undefined;

  try {
    const payload = (await response.json()) as {
      code?: string;
      message?: string;
    };
    message = payload.message ?? message;
    code = payload.code;
  } catch {
    // Preserve the HTTP status text when the body is not JSON.
  }

  throw new ApiError(response.status, response.statusText, message, code);
}

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const response = await fetch(`${BASE_URL}${path}`, init);
  if (!response.ok) {
    return parseError(response);
  }
  return response.json() as Promise<T>;
}

export async function fetchMailboxes(
  accountId = DEFAULT_ACCOUNT_ID,
): Promise<Mailbox[]> {
  return request<Mailbox[]>(`/accounts/${accountId}/mailboxes`);
}

export async function fetchMessages(
  mailboxId: string | null,
  accountId = DEFAULT_ACCOUNT_ID,
): Promise<MessageSummary[]> {
  const search = mailboxId ? `?mailboxId=${encodeURIComponent(mailboxId)}` : "";
  return request<MessageSummary[]>(`/accounts/${accountId}/messages${search}`);
}

export async function fetchMessage(
  messageId: string,
  accountId = DEFAULT_ACCOUNT_ID,
): Promise<MessageDetail> {
  return request<MessageDetail>(`/accounts/${accountId}/messages/${messageId}`);
}

export async function fetchThread(
  threadId: string,
  accountId = DEFAULT_ACCOUNT_ID,
): Promise<ThreadView> {
  return request<ThreadView>(`/accounts/${accountId}/threads/${threadId}`);
}

export async function performMessageCommand(
  messageId: string,
  command: MessageCommand,
  accountId = DEFAULT_ACCOUNT_ID,
): Promise<unknown> {
  switch (command.kind) {
    case "setKeywords":
      return request(
        `/accounts/${accountId}/commands/messages/${messageId}:set-keywords`,
        {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({
            add: command.add,
            remove: command.remove,
          }),
        },
      );
    case "addToMailbox":
      return request(
        `/accounts/${accountId}/commands/messages/${messageId}:add-to-mailbox`,
        {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ mailboxId: command.mailboxId }),
        },
      );
    case "removeFromMailbox":
      return request(
        `/accounts/${accountId}/commands/messages/${messageId}:remove-from-mailbox`,
        {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ mailboxId: command.mailboxId }),
        },
      );
    case "replaceMailboxes":
      return request(
        `/accounts/${accountId}/commands/messages/${messageId}:replace-mailboxes`,
        {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ mailboxIds: command.mailboxIds }),
        },
      );
    case "destroy":
      return request(
        `/accounts/${accountId}/commands/messages/${messageId}:destroy`,
        { method: "POST" },
      );
  }
}

export async function triggerSync(
  accountId = DEFAULT_ACCOUNT_ID,
): Promise<{ ok: boolean; eventCount: number }> {
  return request<{ ok: boolean; eventCount: number }>(
    `/accounts/${accountId}/commands/sync`,
    { method: "POST" },
  );
}

export function buildEventsUrl(accountId = DEFAULT_ACCOUNT_ID): string {
  return `${BASE_URL}/events?accountId=${encodeURIComponent(accountId)}`;
}
