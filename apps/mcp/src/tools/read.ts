import type { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { z } from "zod";

import { apiFetch, type Connection } from "../client.js";
import type { components } from "../schema.gen.js";
import type { ToolWrapper } from "./wrap.js";

type Schemas = components["schemas"];

export function registerReadTools(
  server: McpServer,
  conn: Connection,
  wrap: ToolWrapper,
): void {
  server.registerTool(
    "list_accounts",
    {
      title: "List accounts",
      description: "List all configured accounts with their runtime overview.",
      inputSchema: {},
    },
    wrap(async () => apiFetch<Schemas["AccountOverview"][]>(conn, "/accounts")),
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
}
