// Write operations: each renders one POST /command typed intent. Every write
// mints a fresh idempotency id (ULID) unless the caller passes `id` — passing
// the same id again is a safe retry, never a duplicate. The result is
// `{id, generation}`: acceptance means recorded-and-visible at that
// generation; the provider verdict is observed via list_pending_operations.

import { z } from "zod";

import type {
  Command,
  MessageDetailResult,
  Recipient,
  SendMessageRequest,
} from "@posthaste/protocol/gen";

import type { Connection } from "../core/connection.js";
import { runCommand, runQuery } from "../core/client.js";
import { defineOperation } from "./types.js";

/** The optional retry id every write accepts. */
const idField = {
  id: z
    .string()
    .optional()
    .describe("Idempotency id; pass a previous call's id to retry it safely"),
};

/** Submit an intent and shape the uniform write result. */
async function submit(
  conn: Connection,
  command: Command,
  id: string | undefined,
): Promise<{ id: string; generation: number }> {
  const accepted = await runCommand(conn, command, id);
  return { id: accepted.id, generation: accepted.generation };
}

/** Parse `Name <a@b>` / bare-address strings into typed recipients. */
export function parseRecipient(raw: string): Recipient {
  const match = /^\s*(.*?)\s*<\s*([^<>\s]+@[^<>\s]+)\s*>\s*$/.exec(raw);
  if (match) {
    const name = (match[1] ?? "").replace(/^"|"$/g, "");
    return { name: name.length > 0 ? name : null, email: match[2] ?? "" };
  }
  return { name: null, email: raw.trim() };
}

export const setKeywords = defineOperation({
  mcpName: "set_keywords",
  title: "Set keywords (WRITE)",
  description:
    "WRITE: add and/or remove keywords (tags / read / flagged state) on one message.",
  mutates: true,
  cli: { path: ["tag"], primary: "messageId" },
  argSchema: {
    accountId: z.string().describe("The account the message belongs to"),
    messageId: z.string().describe("The message id"),
    add: z.array(z.string()).optional().describe("Keywords to add (e.g. $seen, $flagged, custom tags)"),
    remove: z.array(z.string()).optional().describe("Keywords to remove"),
    ...idField,
  },
  handler: async (conn, args) => {
    const add = args.add ?? [];
    const remove = args.remove ?? [];
    if (add.length === 0 && remove.length === 0) {
      throw new Error("set_keywords requires at least one keyword to add or remove");
    }
    return submit(
      conn,
      {
        setKeywords: {
          accountId: args.accountId,
          messageId: args.messageId,
          change: { add, remove },
        },
      },
      args.id,
    );
  },
});

export const moveToMailbox = defineOperation({
  mcpName: "move_to_mailbox",
  title: "Move to mailbox (WRITE)",
  description:
    "WRITE: move a message by replacing its mailbox memberships with the given mailbox ids.",
  mutates: true,
  cli: { path: ["move"], primary: "messageId" },
  argSchema: {
    accountId: z.string().describe("The account the message belongs to"),
    messageId: z.string().describe("The message id"),
    mailboxIds: z
      .array(z.string())
      .min(1)
      .describe("The full new mailbox membership (usually one destination id)"),
    ...idField,
  },
  handler: (conn, args) =>
    submit(
      conn,
      {
        replaceMailboxes: {
          accountId: args.accountId,
          messageId: args.messageId,
          change: { mailboxIds: args.mailboxIds },
        },
      },
      args.id,
    ),
});

export const destroyMessage = defineOperation({
  mcpName: "destroy_message",
  title: "Destroy message (WRITE)",
  description: "WRITE: permanently destroy one message (not a move to trash).",
  mutates: true,
  cli: { path: ["messages", "destroy"], primary: "messageId" },
  argSchema: {
    accountId: z.string().describe("The account the message belongs to"),
    messageId: z.string().describe("The message id"),
    ...idField,
  },
  handler: (conn, args) =>
    submit(
      conn,
      { destroy: { accountId: args.accountId, messageId: args.messageId } },
      args.id,
    ),
});

export const createMailbox = defineOperation({
  mcpName: "create_mailbox",
  title: "Create mailbox (WRITE)",
  description: "WRITE: create a top-level mailbox with the given name.",
  mutates: true,
  cli: { path: ["mailboxes", "create"], primary: "name" },
  argSchema: {
    accountId: z.string().describe("The account to create the mailbox in"),
    name: z.string().describe("The mailbox name"),
    ...idField,
  },
  handler: (conn, args) =>
    submit(
      conn,
      { createMailbox: { accountId: args.accountId, name: args.name } },
      args.id,
    ),
});

export const renameMailbox = defineOperation({
  mcpName: "rename_mailbox",
  title: "Rename mailbox (WRITE)",
  description: "WRITE: rename one mailbox.",
  mutates: true,
  cli: { path: ["mailboxes", "rename"] },
  argSchema: {
    accountId: z.string().describe("The account the mailbox belongs to"),
    mailboxId: z.string().describe("The mailbox id"),
    name: z.string().describe("The new name"),
    ...idField,
  },
  handler: (conn, args) =>
    submit(
      conn,
      {
        renameMailbox: {
          accountId: args.accountId,
          mailboxId: args.mailboxId,
          name: args.name,
        },
      },
      args.id,
    ),
});

export const deleteMailbox = defineOperation({
  mcpName: "delete_mailbox",
  title: "Delete mailbox (WRITE)",
  description:
    "WRITE: delete one mailbox. Deleting a non-empty mailbox is refused unless removeEmails is true (the confirm-with-count safety flag).",
  mutates: true,
  cli: { path: ["mailboxes", "delete"] },
  argSchema: {
    accountId: z.string().describe("The account the mailbox belongs to"),
    mailboxId: z.string().describe("The mailbox id"),
    removeEmails: z
      .boolean()
      .optional()
      .describe("Also destroy the mailbox's messages (required for a non-empty mailbox)"),
    ...idField,
  },
  handler: (conn, args) =>
    submit(
      conn,
      {
        deleteMailbox: {
          accountId: args.accountId,
          mailboxId: args.mailboxId,
          removeEmails: args.removeEmails ?? false,
        },
      },
      args.id,
    ),
});

/** The shared compose fields of send_message / reply. */
const composeShape = {
  accountId: z.string().describe("The account to send from"),
  body: z.string().describe("Plain-text message body"),
  cc: z.array(z.string()).optional().describe("Cc recipients (Name <a@b> or bare address)"),
  bcc: z.array(z.string()).optional().describe("Bcc recipients"),
  from: z
    .string()
    .optional()
    .describe("Sender address; defaults to the account's identity"),
};

/** Build the typed request from validated compose args. */
function sendRequest(args: {
  body: string;
  subject: string;
  to: string[];
  cc?: string[] | undefined;
  bcc?: string[] | undefined;
  from?: string | undefined;
  inReplyTo?: string | null;
  references?: string | null;
}): SendMessageRequest {
  return {
    from: args.from ? parseRecipient(args.from) : null,
    to: args.to.map(parseRecipient),
    cc: (args.cc ?? []).map(parseRecipient),
    bcc: (args.bcc ?? []).map(parseRecipient),
    subject: args.subject,
    body: args.body,
    inReplyTo: args.inReplyTo ?? null,
    references: args.references ?? null,
    attachments: [],
    draftId: null,
  };
}

export const sendMessage = defineOperation({
  mcpName: "send_message",
  title: "Send message (WRITE)",
  description:
    "WRITE: send a new message (not in reply to anything — use reply for in-thread).",
  mutates: true,
  cli: { path: ["send"] },
  argSchema: {
    to: z.array(z.string()).min(1).describe("To recipients (Name <a@b> or bare address)"),
    subject: z.string().describe("Subject line"),
    ...composeShape,
    ...idField,
  },
  handler: (conn, args) =>
    submit(
      conn,
      { send: { accountId: args.accountId, request: sendRequest(args) } },
      args.id,
    ),
});

export const reply = defineOperation({
  mcpName: "reply",
  title: "Reply to message (WRITE)",
  description:
    "WRITE: reply in-thread to a message — reads the original for the recipient, subject, and threading headers, then sends the body through them.",
  mutates: true,
  cli: { path: ["reply"], primary: "messageId" },
  argSchema: {
    messageId: z.string().describe("The message to reply to"),
    ...composeShape,
    to: z
      .array(z.string())
      .optional()
      .describe("Override recipients; defaults to the original sender"),
    subject: z.string().optional().describe("Override subject; defaults to Re: <original>"),
    ...idField,
  },
  handler: async (conn, args) => {
    const detail = await runQuery<MessageDetailResult>(conn, {
      messageDetail: { accountId: args.accountId, messageId: args.messageId },
    });
    const summary = detail.data.summary;

    let to = args.to;
    if (!to || to.length === 0) {
      if (!summary.fromEmail) {
        throw new Error(
          "the original message has no sender address; pass --to explicitly",
        );
      }
      to = [
        summary.fromName
          ? `${summary.fromName} <${summary.fromEmail}>`
          : summary.fromEmail,
      ];
    }
    const originalSubject = summary.subject ?? "";
    const subject =
      args.subject ??
      (/^re:/i.test(originalSubject) ? originalSubject : `Re: ${originalSubject}`);
    const rfcId = summary.rfcMessageId ?? null;
    // References: the original's In-Reply-To chain tail + its own id — the
    // best chain reconstructable from the summary projection.
    const references =
      [summary.inReplyTo, rfcId].filter((v): v is string => !!v).join(" ") || null;

    return submit(
      conn,
      {
        send: {
          accountId: args.accountId,
          request: sendRequest({ ...args, to, subject, inReplyTo: rfcId, references }),
        },
      },
      args.id,
    );
  },
});

export const triggerSync = defineOperation({
  mcpName: "trigger_sync",
  title: "Trigger sync (WRITE)",
  description:
    "WRITE: start a sync for one account now. Progress and completion surface as events and account status.",
  mutates: true,
  cli: { path: ["sync"], primary: "accountId" },
  argSchema: {
    accountId: z.string().describe("The account to sync"),
    mode: z
      .enum(["incremental", "fullMetadata"])
      .optional()
      .describe("Sync depth; defaults to incremental"),
    ...idField,
  },
  handler: (conn, args) =>
    submit(
      conn,
      { syncNow: { accountId: args.accountId, mode: args.mode ?? null } },
      args.id,
    ),
});

export const writeOperations = [
  setKeywords,
  moveToMailbox,
  destroyMessage,
  createMailbox,
  renameMailbox,
  deleteMailbox,
  sendMessage,
  reply,
  triggerSync,
];
