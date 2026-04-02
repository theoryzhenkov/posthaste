import { ApiError } from "./errors";
import type {
  AccountOverview,
  AppSettings,
  ConversationPage,
  ConversationView,
  CreateAccountInput,
  CreateSmartMailboxInput,
  Mailbox,
  MessageCommand,
  MessageCommandResult,
  MessageDetail,
  MessageSummary,
  OkResponse,
  SidebarResponse,
  SmartMailbox,
  SmartMailboxSummary,
  UpdateAccountInput,
  UpdateSmartMailboxInput,
  VerificationResponse,
} from "./types";

function normalizeApiBaseUrl(baseUrl: string): string {
  return baseUrl.replace(/\/+$/, "");
}

const BASE_URL = normalizeApiBaseUrl(
  import.meta.env.VITE_API_BASE_URL?.trim() || "http://localhost:3001/v1",
);

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

function jsonRequest<T>(path: string, method: string, body?: unknown): Promise<T> {
  return request<T>(path, {
    method,
    headers: { "Content-Type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
}

export async function fetchSettings(): Promise<AppSettings> {
  return request<AppSettings>("/settings");
}

export async function patchSettings(
  input: Partial<AppSettings>,
): Promise<AppSettings> {
  return jsonRequest<AppSettings>("/settings", "PATCH", input);
}

export async function fetchAccounts(): Promise<AccountOverview[]> {
  return request<AccountOverview[]>("/accounts");
}

export async function fetchAccount(accountId: string): Promise<AccountOverview> {
  return request<AccountOverview>(`/accounts/${accountId}`);
}

export async function createAccount(
  input: CreateAccountInput,
): Promise<AccountOverview> {
  return jsonRequest<AccountOverview>("/accounts", "POST", input);
}

export async function updateAccount(
  accountId: string,
  input: UpdateAccountInput,
): Promise<AccountOverview> {
  return jsonRequest<AccountOverview>(`/accounts/${accountId}`, "PATCH", input);
}

export async function deleteAccount(accountId: string): Promise<OkResponse> {
  return request<OkResponse>(`/accounts/${accountId}`, { method: "DELETE" });
}

export async function verifyAccount(
  accountId: string,
): Promise<VerificationResponse> {
  return request<VerificationResponse>(`/accounts/${accountId}/verify`, {
    method: "POST",
  });
}

export async function enableAccount(accountId: string): Promise<OkResponse> {
  return request<OkResponse>(`/accounts/${accountId}/enable`, { method: "POST" });
}

export async function disableAccount(accountId: string): Promise<OkResponse> {
  return request<OkResponse>(`/accounts/${accountId}/disable`, { method: "POST" });
}

export async function fetchMailboxes(accountId: string): Promise<Mailbox[]> {
  return request<Mailbox[]>(`/sources/${accountId}/mailboxes`);
}

export async function fetchSidebar(): Promise<SidebarResponse> {
  return request<SidebarResponse>("/sidebar");
}

export async function fetchSmartMailboxes(): Promise<SmartMailboxSummary[]> {
  return request<SmartMailboxSummary[]>("/smart-mailboxes");
}

export async function createSmartMailbox(
  input: CreateSmartMailboxInput,
): Promise<SmartMailbox> {
  return jsonRequest<SmartMailbox>("/smart-mailboxes", "POST", input);
}

export async function fetchSmartMailbox(id: string): Promise<SmartMailbox> {
  return request<SmartMailbox>(`/smart-mailboxes/${id}`);
}

export async function updateSmartMailbox(
  id: string,
  input: UpdateSmartMailboxInput,
): Promise<SmartMailbox> {
  return jsonRequest<SmartMailbox>(`/smart-mailboxes/${id}`, "PATCH", input);
}

export async function deleteSmartMailbox(id: string): Promise<OkResponse> {
  return request<OkResponse>(`/smart-mailboxes/${id}`, { method: "DELETE" });
}

export async function resetDefaultSmartMailboxes(): Promise<SmartMailboxSummary[]> {
  return request<SmartMailboxSummary[]>("/smart-mailboxes:reset-defaults", {
    method: "POST",
  });
}

export async function fetchSmartMailboxMessages(
  id: string,
): Promise<MessageSummary[]> {
  return request<MessageSummary[]>(`/smart-mailboxes/${id}/messages`);
}

export async function fetchSmartMailboxConversations(
  id: string,
  input?: { limit?: number; cursor?: string | null },
): Promise<ConversationPage> {
  const params = new URLSearchParams();
  if (input?.limit !== undefined) {
    params.set("limit", String(input.limit));
  }
  if (input?.cursor) {
    params.set("cursor", input.cursor);
  }
  const search = params.toString();
  return request<ConversationPage>(
    `/smart-mailboxes/${id}/conversations${search ? `?${search}` : ""}`,
  );
}

export async function fetchConversations(input?: {
  sourceId?: string | null;
  mailboxId?: string | null;
  limit?: number;
  cursor?: string | null;
}): Promise<ConversationPage> {
  const params = new URLSearchParams();
  if (input?.sourceId) {
    params.set("sourceId", input.sourceId);
  }
  if (input?.mailboxId) {
    params.set("mailboxId", input.mailboxId);
  }
  if (input?.limit !== undefined) {
    params.set("limit", String(input.limit));
  }
  if (input?.cursor) {
    params.set("cursor", input.cursor);
  }
  const search = params.toString();
  return request<ConversationPage>(
    `/views/conversations${search ? `?${search}` : ""}`,
  );
}

export async function fetchConversation(
  conversationId: string,
): Promise<ConversationView> {
  return request<ConversationView>(`/views/conversations/${conversationId}`);
}

export async function fetchMessage(
  messageId: string,
  sourceId: string,
): Promise<MessageDetail> {
  return request<MessageDetail>(`/sources/${sourceId}/messages/${messageId}`);
}

export async function fetchSourceMessages(
  sourceId: string,
  mailboxId: string | null,
): Promise<MessageSummary[]> {
  const search = mailboxId ? `?mailboxId=${encodeURIComponent(mailboxId)}` : "";
  return request<MessageSummary[]>(`/sources/${sourceId}/messages${search}`);
}

export async function performMessageCommand(
  messageId: string,
  command: MessageCommand,
  sourceId: string,
): Promise<MessageCommandResult> {
  switch (command.kind) {
    case "setKeywords":
      return jsonRequest<MessageCommandResult>(
        `/sources/${sourceId}/commands/messages/${messageId}/set-keywords`,
        "POST",
        {
          add: command.add,
          remove: command.remove,
        },
      );
    case "addToMailbox":
      return jsonRequest<MessageCommandResult>(
        `/sources/${sourceId}/commands/messages/${messageId}/add-to-mailbox`,
        "POST",
        { mailboxId: command.mailboxId },
      );
    case "removeFromMailbox":
      return jsonRequest<MessageCommandResult>(
        `/sources/${sourceId}/commands/messages/${messageId}/remove-from-mailbox`,
        "POST",
        { mailboxId: command.mailboxId },
      );
    case "replaceMailboxes":
      return jsonRequest<MessageCommandResult>(
        `/sources/${sourceId}/commands/messages/${messageId}/replace-mailboxes`,
        "POST",
        { mailboxIds: command.mailboxIds },
      );
    case "destroy":
      return request<MessageCommandResult>(`/sources/${sourceId}/commands/messages/${messageId}/destroy`, {
        method: "POST",
      });
  }
}

export async function triggerSync(
  sourceId: string,
): Promise<{ ok: boolean; eventCount: number }> {
  return request<{ ok: boolean; eventCount: number }>(
    `/sources/${sourceId}/commands/sync`,
    { method: "POST" },
  );
}

export function buildEventsUrl(input?: {
  accountId?: string;
  afterSeq?: number | null;
}): string {
  const params = new URLSearchParams();
  if (input?.accountId) {
    params.set("accountId", input.accountId);
  }
  if (input?.afterSeq != null) {
    params.set("afterSeq", String(input.afterSeq));
  }
  const search = params.toString();
  return `${BASE_URL}/events${search ? `?${search}` : ""}`;
}
