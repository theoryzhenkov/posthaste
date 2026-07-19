// Read operations: each renders one POST /query family. Results are returned
// as the full envelope ({generation, data}) so scripts and agents can
// correlate answers with the event stream's generations.

import { writeFile } from "node:fs/promises";

import { z } from "zod";

import type {
  AccountSettingsResult,
  AccountsResult,
  AppSettingsResult,
  MailListQuery,
  MailListResult,
  MailboxCountsResult,
  MessageDetailResult,
  PendingOperationsResult,
  ThreadView,
} from "@posthaste/protocol/gen";

import { fetchBlob, runQuery } from "../core/client.js";
import { defineOperation } from "./types.js";

/**
 * Largest blob returned inline as base64. Bigger blobs must go to a file via
 * `output` — an unbounded base64 answer would flood an agent's context window
 * (and a shell pipeline) with megabytes of attachment bytes.
 */
const MAX_INLINE_BLOB_BYTES = 4 * 1024 * 1024;

/** Shared windowing/filter fields for the mail-list family. */
const mailListShape = {
  accountId: z.string().optional().describe("Restrict to one account"),
  mailboxId: z
    .string()
    .optional()
    .describe("Restrict to one mailbox (pair with accountId)"),
  isRead: z.boolean().optional().describe("Only read (true) or unread (false)"),
  isFlagged: z.boolean().optional().describe("Only flagged (true) or unflagged (false)"),
  hasAttachment: z.boolean().optional().describe("Only with (true) or without (false) attachments"),
  limit: z.number().int().positive().optional().describe("Max rows per page (server-capped)"),
  cursor: z.string().optional().describe("Opaque continuation from the previous page"),
};

/** Assemble the typed mail-list query from validated args. */
function mailListQuery(args: {
  accountId?: string | undefined;
  mailboxId?: string | undefined;
  freeText?: string | undefined;
  isRead?: boolean | undefined;
  isFlagged?: boolean | undefined;
  hasAttachment?: boolean | undefined;
  limit?: number | undefined;
  cursor?: string | undefined;
}): MailListQuery {
  return {
    accountId: args.accountId ?? null,
    mailboxId: args.mailboxId ?? null,
    freeText: args.freeText ?? null,
    isRead: args.isRead ?? null,
    isFlagged: args.isFlagged ?? null,
    hasAttachment: args.hasAttachment ?? null,
    limit: args.limit ?? null,
    cursor: args.cursor ?? null,
  };
}

export const listAccounts = defineOperation({
  mcpName: "list_accounts",
  title: "List accounts",
  description:
    "List every configured mail account with its live health (sync status, push state, last error).",
  mutates: false,
  cli: { path: ["accounts", "list"] },
  argSchema: {},
  handler: (conn) => runQuery<AccountsResult>(conn, { accounts: {} }),
});

export const listMailboxes = defineOperation({
  mcpName: "list_mailboxes",
  title: "List mailboxes",
  description:
    "List mailboxes with unread/total counts, in display order, optionally for one account.",
  mutates: false,
  cli: { path: ["mailboxes", "list"] },
  argSchema: {
    accountId: z.string().optional().describe("Restrict to one account"),
  },
  handler: (conn, args) =>
    runQuery<MailboxCountsResult>(conn, {
      mailboxCounts: { accountId: args.accountId ?? null },
    }),
});

export const listMessages = defineOperation({
  mcpName: "list_messages",
  title: "List messages",
  description:
    "List messages as a windowed page (never the whole mailbox): filters, a limit, and an opaque cursor for the next page.",
  mutates: false,
  cli: { path: ["messages", "list"] },
  argSchema: mailListShape,
  handler: (conn, args) =>
    runQuery<MailListResult>(conn, { mailList: mailListQuery(args) }),
});

export const searchMessages = defineOperation({
  mcpName: "search_messages",
  title: "Search messages",
  description:
    "Full-text search over mail, returned as a windowed mail list. The query grammar supports prefixed tokens (from:, is:, conversation:, ...) and bare words matching sender, subject, preview, and body.",
  mutates: false,
  cli: { path: ["messages", "search"], primary: "query" },
  argSchema: {
    query: z.string().describe("Search text in the query grammar"),
    ...mailListShape,
  },
  handler: (conn, args) =>
    runQuery<MailListResult>(conn, {
      mailList: mailListQuery({ ...args, freeText: args.query }),
    }),
});

export const getMessage = defineOperation({
  mcpName: "get_message",
  title: "Get message",
  description:
    "Read one message: summary, sanitized text/HTML bodies, and attachment metadata (attachment bytes are separate blob fetches).",
  mutates: false,
  cli: { path: ["messages", "get"], primary: "messageId" },
  argSchema: {
    accountId: z.string().describe("The account the message belongs to"),
    messageId: z.string().describe("The message id"),
  },
  handler: (conn, args) =>
    runQuery<MessageDetailResult>(conn, {
      messageDetail: { accountId: args.accountId, messageId: args.messageId },
    }),
});

export const getThread = defineOperation({
  mcpName: "get_thread",
  title: "Get thread",
  description:
    "Read a whole conversation thread: every message summary of one provider thread, in order.",
  mutates: false,
  cli: { path: ["threads", "get"], primary: "threadId" },
  argSchema: {
    accountId: z.string().describe("The account the thread belongs to"),
    threadId: z.string().describe("The provider thread id (sourceThreadId on a message summary)"),
  },
  handler: (conn, args) =>
    runQuery<ThreadView>(conn, {
      thread: { accountId: args.accountId, threadId: args.threadId },
    }),
});

export const getBlob = defineOperation({
  mcpName: "get_blob",
  title: "Get blob",
  description:
    "Fetch one immutable blob's bytes (attachment content) by the blobId from a message's attachment metadata. Writes to `output` when given; otherwise returns the bytes inline as base64 (small blobs only — pass `output` for anything big).",
  mutates: false,
  cli: { path: ["blobs", "get"], primary: "blobId" },
  argSchema: {
    blobId: z.string().describe("The blob id (attachment metadata's blobId)"),
    output: z
      .string()
      .optional()
      .describe("File path to write the bytes to; omit to get base64 inline"),
  },
  handler: async (conn, args) => {
    const bytes = await fetchBlob(conn, args.blobId);
    if (args.output !== undefined) {
      await writeFile(args.output, bytes);
      return { blobId: args.blobId, byteLength: bytes.length, output: args.output };
    }
    if (bytes.length > MAX_INLINE_BLOB_BYTES) {
      throw new Error(
        `blob is ${bytes.length} bytes, over the ${MAX_INLINE_BLOB_BYTES}-byte inline limit; pass --output <path> to write it to a file`,
      );
    }
    return {
      blobId: args.blobId,
      byteLength: bytes.length,
      base64: Buffer.from(bytes).toString("base64"),
    };
  },
});

export const listPendingOperations = defineOperation({
  mcpName: "list_pending_operations",
  title: "List pending operations",
  description:
    "Read the outbox: accepted commands awaiting or past provider settlement, with state, attempts, and last error — how a caller observes a command's eventual verdict.",
  mutates: false,
  cli: { path: ["operations", "list"] },
  argSchema: {
    accountId: z.string().optional().describe("Restrict to one account"),
  },
  handler: (conn, args) =>
    runQuery<PendingOperationsResult>(conn, {
      pendingOperations: { accountId: args.accountId ?? null },
    }),
});

export const getAppSettings = defineOperation({
  mcpName: "get_app_settings",
  title: "Get app settings",
  description: "Read the application settings tree (appearance, compose, notifications).",
  mutates: false,
  cli: { path: ["settings", "get"] },
  argSchema: {},
  handler: (conn) => runQuery<AppSettingsResult>(conn, { appSettings: {} }),
});

export const getAccountSettings = defineOperation({
  mcpName: "get_account_settings",
  title: "Get account settings",
  description:
    "Read one account's settings (identity, transport views, secret status — never secret material).",
  mutates: false,
  cli: { path: ["accounts", "settings"], primary: "accountId" },
  argSchema: {
    accountId: z.string().describe("The account id"),
  },
  handler: (conn, args) =>
    runQuery<AccountSettingsResult>(conn, {
      accountSettings: { accountId: args.accountId },
    }),
});

export const readOperations = [
  listAccounts,
  listMailboxes,
  listMessages,
  searchMessages,
  getMessage,
  getThread,
  getBlob,
  listPendingOperations,
  getAppSettings,
  getAccountSettings,
];
