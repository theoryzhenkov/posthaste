import { z } from "zod";

import { apiFetch } from "../client.js";
import type { components } from "../schema.gen.js";
import { defineOperation, type Operation } from "./types.js";

type Schemas = components["schemas"];

const recipientSchema = z.object({
  email: z.string(),
  name: z.string().nullish(),
});
const attachmentSchema = z.object({
  filename: z.string(),
  mimeType: z.string(),
  contentBase64: z.string(),
});

/**
 * The mutating operations. The first three are the original `tools/commands.ts`
 * handlers, unchanged; `list_mailboxes` and `trigger_sync` are additive thin
 * wrappers over existing `/v1` endpoints — the "one vocabulary" principle
 * (docs/eph/RFC-L2-scripting.md §6, D53, the action path).
 */
export const commandOperations: Operation[] = [
  defineOperation({
    mcpName: "list_mailboxes",
    title: "List mailboxes",
    description:
      "List a source's mailboxes (folders/labels) with their summaries.",
    mutates: false,
    cli: { path: ["mailboxes", "list"], primary: "sourceId" },
    argSchema: {
      sourceId: z.string(),
    },
    handler: (conn, args) =>
      apiFetch<Schemas["MailboxSummary"][]>(
        conn,
        `/sources/${encodeURIComponent(args.sourceId)}/mailboxes`,
      ),
  }),

  defineOperation({
    mcpName: "set_keywords",
    title: "Set keywords",
    description:
      "Add and/or remove JMAP keywords (flags) on a message. Both 'add' and " +
      "'remove' are arrays (pass [] for none).",
    mutates: true,
    cli: { path: ["messages", "set-keywords"] },
    argSchema: {
      sourceId: z.string(),
      messageId: z.string(),
      add: z.array(z.string()),
      remove: z.array(z.string()),
    },
    handler: (conn, args) => {
      const body: Schemas["SetKeywordsCommand"] = {
        add: args.add,
        remove: args.remove,
      };
      return apiFetch<unknown>(
        conn,
        `/sources/${encodeURIComponent(args.sourceId)}/commands/messages/${encodeURIComponent(args.messageId)}/set-keywords`,
        { method: "POST", body },
      );
    },
  }),

  defineOperation({
    mcpName: "move_to_mailbox",
    title: "Move to mailbox",
    description:
      "Add a message to an additional mailbox (folder/label) by mailbox id.",
    mutates: true,
    cli: { path: ["messages", "add-to-mailbox"] },
    argSchema: {
      sourceId: z.string(),
      messageId: z.string(),
      mailboxId: z.string(),
    },
    handler: (conn, args) => {
      const body: Schemas["AddToMailboxCommand"] = {
        mailboxId: args.mailboxId,
      };
      return apiFetch<unknown>(
        conn,
        `/sources/${encodeURIComponent(args.sourceId)}/commands/messages/${encodeURIComponent(args.messageId)}/add-to-mailbox`,
        { method: "POST", body },
      );
    },
  }),

  defineOperation({
    mcpName: "send_message",
    title: "Send message",
    description:
      "Send a new email via the source's submission identity. 'to', 'cc', " +
      "'bcc' are arrays of {email, name?}; 'subject' and 'body' are required. " +
      "attachments are optional {filename, mimeType, contentBase64} objects.",
    mutates: true,
    cli: { path: ["messages", "send"] },
    argSchema: {
      sourceId: z.string(),
      to: z.array(recipientSchema),
      cc: z.array(recipientSchema),
      bcc: z.array(recipientSchema),
      subject: z.string(),
      body: z.string(),
      from: recipientSchema.nullish(),
      inReplyTo: z.string().nullish(),
      references: z.string().nullish(),
      attachments: z.array(attachmentSchema).optional(),
    },
    handler: (conn, args) => {
      const body: Schemas["SendMessageRequest"] = {
        to: args.to,
        cc: args.cc,
        bcc: args.bcc,
        subject: args.subject,
        body: args.body,
        from: args.from ?? undefined,
        inReplyTo: args.inReplyTo ?? undefined,
        references: args.references ?? undefined,
        attachments: args.attachments ?? [],
      };
      return apiFetch<unknown>(
        conn,
        `/sources/${encodeURIComponent(args.sourceId)}/commands/send`,
        { method: "POST", body },
      );
    },
  }),

  defineOperation({
    mcpName: "trigger_sync",
    title: "Trigger sync",
    description:
      "Run a manual sync for a source and report the number of events emitted. " +
      "'mode' is 'incremental' (default) or 'fullMetadata'.",
    mutates: true,
    cli: { path: ["sync"], primary: "sourceId" },
    argSchema: {
      sourceId: z.string(),
      mode: z.enum(["incremental", "fullMetadata"]).optional(),
    },
    handler: (conn, args) => {
      const body: Schemas["TriggerSyncRequest"] = { mode: args.mode };
      return apiFetch<Schemas["TriggerSyncResponse"]>(
        conn,
        `/sources/${encodeURIComponent(args.sourceId)}/commands/sync`,
        { method: "POST", body },
      );
    },
  }),
];
