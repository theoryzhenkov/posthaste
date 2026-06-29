import { z } from "zod";

import { apiFetch } from "../client.js";
import type { components } from "../schema.gen.js";
import { defineOperation, type Operation } from "./types.js";

type Schemas = components["schemas"];

/**
 * The read (non-mutating) operations. Each maps 1:1 to one documented `/v1`
 * read operation; the handler bodies are the original `tools/read.ts` calls,
 * unchanged, now front-end-agnostic.
 */
export const readOperations: Operation[] = [
  defineOperation({
    mcpName: "list_accounts",
    title: "List accounts",
    description: "List all configured accounts with their runtime overview.",
    mutates: false,
    cli: { path: ["accounts", "list"] },
    argSchema: {},
    handler: (conn) =>
      apiFetch<Schemas["AccountOverview"][]>(conn, "/accounts"),
  }),

  defineOperation({
    mcpName: "read_mail_navigation",
    title: "Read mail navigation",
    description:
      "Read accounts, enabled-account mailboxes, smart mailboxes, and tags in one typed batch.",
    mutates: false,
    cli: { path: ["nav"] },
    argSchema: {},
    handler: (conn) =>
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
  }),

  defineOperation({
    mcpName: "list_conversations",
    title: "List conversations",
    description:
      "List conversations in the unified or filtered view. Optionally scope " +
      "by sourceId/mailboxId, full-text query, page size, and cursor.",
    mutates: false,
    cli: { path: ["conversations", "list"] },
    argSchema: {
      sourceId: z.string().optional(),
      mailboxId: z.string().optional(),
      q: z.string().optional(),
      limit: z.number().int().positive().optional(),
      cursor: z.string().optional(),
    },
    handler: (conn, args) =>
      apiFetch<Schemas["ConversationPageResponse"]>(
        conn,
        "/views/conversations",
        {
          query: {
            sourceId: args.sourceId,
            mailboxId: args.mailboxId,
            q: args.q,
            limit: args.limit,
            cursor: args.cursor,
          },
        },
      ),
  }),

  defineOperation({
    mcpName: "get_conversation",
    title: "Get conversation",
    description:
      "Get a single conversation thread (its messages) by conversation id.",
    mutates: false,
    cli: { path: ["conversations", "get"], primary: "conversationId" },
    argSchema: {
      conversationId: z.string(),
    },
    handler: (conn, args) =>
      apiFetch<Schemas["ConversationView"]>(
        conn,
        `/views/conversations/${encodeURIComponent(args.conversationId)}`,
      ),
  }),

  defineOperation({
    mcpName: "search_messages",
    title: "Search messages",
    description:
      "Full-text search messages across accounts. 'q' is required; supports " +
      "page size and cursor.",
    mutates: false,
    cli: { path: ["messages", "search"], primary: "q" },
    argSchema: {
      q: z.string(),
      limit: z.number().int().positive().optional(),
      cursor: z.string().optional(),
    },
    handler: (conn, args) =>
      apiFetch<Schemas["MessagePageResponse"]>(conn, "/messages/search", {
        query: { q: args.q, limit: args.limit, cursor: args.cursor },
      }),
  }),

  defineOperation({
    mcpName: "get_message",
    title: "Get message",
    description:
      "Get a single message's full content by source and message id.",
    mutates: false,
    cli: { path: ["messages", "get"] },
    argSchema: {
      sourceId: z.string(),
      messageId: z.string(),
    },
    handler: (conn, args) =>
      apiFetch<Schemas["MessageDetail"]>(
        conn,
        `/sources/${encodeURIComponent(args.sourceId)}/messages/${encodeURIComponent(args.messageId)}`,
      ),
  }),
];
