import type { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { z } from "zod";

import { apiFetch, type Connection } from "../client.js";
import type { components } from "../schema.gen.js";
import type { ToolWrapper } from "./wrap.js";

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

export function registerCommandTools(
  server: McpServer,
  conn: Connection,
  wrap: ToolWrapper,
): void {
  server.registerTool(
    "set_keywords",
    {
      title: "Set keywords",
      description:
        "Add and/or remove JMAP keywords (flags) on a message. Both 'add' and " +
        "'remove' are arrays (pass [] for none).",
      inputSchema: {
        sourceId: z.string(),
        messageId: z.string(),
        add: z.array(z.string()),
        remove: z.array(z.string()),
      },
    },
    wrap(async (args) => {
      const body: Schemas["SetKeywordsCommand"] = {
        add: args.add,
        remove: args.remove,
      };
      return apiFetch<unknown>(
        conn,
        `/sources/${encodeURIComponent(args.sourceId)}/commands/messages/${encodeURIComponent(args.messageId)}/set-keywords`,
        { method: "POST", body },
      );
    }),
  );

  server.registerTool(
    "move_to_mailbox",
    {
      title: "Move to mailbox",
      description:
        "Add a message to an additional mailbox (folder/label) by mailbox id.",
      inputSchema: {
        sourceId: z.string(),
        messageId: z.string(),
        mailboxId: z.string(),
      },
    },
    wrap(async (args) => {
      const body: Schemas["AddToMailboxCommand"] = { mailboxId: args.mailboxId };
      return apiFetch<unknown>(
        conn,
        `/sources/${encodeURIComponent(args.sourceId)}/commands/messages/${encodeURIComponent(args.messageId)}/add-to-mailbox`,
        { method: "POST", body },
      );
    }),
  );

  server.registerTool(
    "send_message",
    {
      title: "Send message",
      description:
        "Send a new email via the source's submission identity. 'to', 'cc', " +
        "'bcc' are arrays of {email, name?}; 'subject' and 'body' are required. " +
        "attachments are optional {filename, mimeType, contentBase64} objects.",
      inputSchema: {
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
    },
    wrap(async (args) => {
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
    }),
  );
}
