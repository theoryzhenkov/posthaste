#!/usr/bin/env bun
import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { StdioServerTransport } from "@modelcontextprotocol/sdk/server/stdio.js";
import { z } from "zod";

import {
  apiFetch,
  ApiError,
  ConnectionError,
  resolveConnection,
  type Connection,
} from "./client.js";
import type { components } from "./schema.gen.js";

type Schemas = components["schemas"];

/**
 * Build the MCP server and register the tool set. Each tool maps 1:1 to one
 * documented `/v1` operation; the daemon does the work, this is a thin adapter.
 */
function buildServer(conn: Connection): McpServer {
  const server = new McpServer(
    { name: "posthaste-mcp", version: "0.0.0" },
    { capabilities: { tools: {} } },
  );

  /**
   * Wrap a tool body so a successful result is returned as JSON text content
   * and an API/connection failure becomes an MCP tool error (isError) rather
   * than crashing the transport.
   */
  const wrap =
    <Args>(fn: (args: Args) => Promise<unknown>) =>
    async (args: Args) => {
      try {
        const result = await fn(args);
        return {
          content: [
            {
              type: "text" as const,
              text: JSON.stringify(result ?? null, null, 2),
            },
          ],
        };
      } catch (error) {
        const message =
          error instanceof ApiError || error instanceof ConnectionError
            ? error.message
            : error instanceof Error
              ? error.message
              : String(error);
        return {
          isError: true,
          content: [{ type: "text" as const, text: message }],
        };
      }
    };

  // --- Read operations ----------------------------------------------------

  server.registerTool(
    "list_accounts",
    {
      title: "List accounts",
      description: "List all configured accounts with their runtime overview.",
      inputSchema: {},
    },
    wrap(async () =>
      apiFetch<Schemas["AccountOverview"][]>(conn, "/accounts"),
    ),
  );

  server.registerTool(
    "read_mail_navigation",
    {
      title: "Read mail navigation",
      description:
        "Read accounts, enabled-account mailboxes, smart mailboxes, and tags in one typed batch.",
      inputSchema: {},
    },
    wrap(async () =>
      apiFetch<Schemas["ReadResponse"]>(conn, "/read", {
        method: "POST",
        body: {
          calls: [
            { id: "accounts", op: "Account/list" },
            {
              id: "mailboxes",
              op: "Mailbox/list",
              args: { accountIds: "#accounts.enabledIds" },
            },
            { id: "smartMailboxes", op: "SmartMailbox/list" },
            {
              id: "tags",
              op: "Tag/list",
              args: { accountIds: "#accounts.enabledIds" },
            },
          ],
        },
      }),
    ),
  );

  server.registerTool(
    "list_conversations",
    {
      title: "List conversations",
      description:
        "List conversations in the unified or filtered view. Optionally scope " +
        "by sourceId/mailboxId, full-text query, page size, and cursor.",
      inputSchema: {
        sourceId: z.string().optional(),
        mailboxId: z.string().optional(),
        q: z.string().optional(),
        limit: z.number().int().positive().optional(),
        cursor: z.string().optional(),
      },
    },
    wrap(async (args) =>
      apiFetch<Schemas["ConversationPageResponse"]>(conn, "/views/conversations", {
        query: {
          sourceId: args.sourceId,
          mailboxId: args.mailboxId,
          q: args.q,
          limit: args.limit,
          cursor: args.cursor,
        },
      }),
    ),
  );

  server.registerTool(
    "get_conversation",
    {
      title: "Get conversation",
      description:
        "Get a single conversation thread (its messages) by conversation id.",
      inputSchema: {
        conversationId: z.string(),
      },
    },
    wrap(async (args) =>
      apiFetch<Schemas["ConversationView"]>(
        conn,
        `/views/conversations/${encodeURIComponent(args.conversationId)}`,
      ),
    ),
  );

  server.registerTool(
    "search_messages",
    {
      title: "Search messages",
      description:
        "Full-text search messages across accounts. 'q' is required; supports " +
        "page size and cursor.",
      inputSchema: {
        q: z.string(),
        limit: z.number().int().positive().optional(),
        cursor: z.string().optional(),
      },
    },
    wrap(async (args) =>
      apiFetch<Schemas["MessagePageResponse"]>(conn, "/messages/search", {
        query: { q: args.q, limit: args.limit, cursor: args.cursor },
      }),
    ),
  );

  server.registerTool(
    "get_message",
    {
      title: "Get message",
      description: "Get a single message's full content by source and message id.",
      inputSchema: {
        sourceId: z.string(),
        messageId: z.string(),
      },
    },
    wrap(async (args) =>
      apiFetch<Schemas["MessageDetail"]>(
        conn,
        `/sources/${encodeURIComponent(args.sourceId)}/messages/${encodeURIComponent(args.messageId)}`,
      ),
    ),
  );

  // --- Mutating commands --------------------------------------------------

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

  const recipientSchema = z.object({
    email: z.string(),
    name: z.string().nullish(),
  });
  const attachmentSchema = z.object({
    filename: z.string(),
    mimeType: z.string(),
    contentBase64: z.string(),
  });

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

  return server;
}

async function main(): Promise<void> {
  let conn: Connection;
  try {
    conn = resolveConnection();
  } catch (error) {
    const message =
      error instanceof ConnectionError ? error.message : String(error);
    process.stderr.write(`posthaste-mcp: ${message}\n`);
    process.exit(1);
  }

  process.stderr.write(
    `posthaste-mcp: connected to ${conn.baseUrl} (via ${conn.source}, ` +
      `${conn.token ? "with token" : "no token"})\n`,
  );

  const server = buildServer(conn);
  const transport = new StdioServerTransport();
  await server.connect(transport);
}

// Only start the stdio server when run as the entry module, so importing this
// module (e.g. from a smoke test) does not block on the transport loop.
if (import.meta.main) {
  main().catch((error) => {
    process.stderr.write(`posthaste-mcp: fatal: ${String(error)}\n`);
    process.exit(1);
  });
}

export { buildServer, main };
