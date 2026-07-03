#!/usr/bin/env bun
import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { StdioServerTransport } from "@modelcontextprotocol/sdk/server/stdio.js";

import {
  ApiError,
  ConnectionError,
  resolveConnection,
  type Connection,
} from "./client.js";
import { mintConnectionToken } from "./connect.js";
import { operations } from "./operations/index.js";
import { subscribeEvents } from "./subscription.js";

/**
 * Render an operation's result (or error) as an MCP tool result. JSON text
 * content on success; the typed `ApiErrorBody` message as a tool error on
 * failure. (This is the MCP front-end's rendering of the shared registry; the
 * CLI renders the same operations to stdout/exit-codes instead.)
 */
async function runAsTool(
  fn: () => Promise<unknown>,
): Promise<{ content: { type: "text"; text: string }[]; isError?: true }> {
  try {
    const result = await fn();
    return {
      content: [
        { type: "text", text: JSON.stringify(result ?? null, null, 2) },
      ],
    };
  } catch (error) {
    const message =
      error instanceof ApiError || error instanceof ConnectionError
        ? error.message
        : error instanceof Error
          ? error.message
          : String(error);
    return { isError: true, content: [{ type: "text", text: message }] };
  }
}

/**
 * Build the MCP server and register the operation registry as tools. Each tool
 * maps 1:1 to one documented `/v1` operation; the daemon does the work, this is
 * a thin adapter. The `readOnlyHint` annotation comes from the operation's
 * `mutates` flag.
 */
function buildServer(conn: Connection): McpServer {
  const server = new McpServer(
    { name: "posthaste-mcp", version: "0.0.0" },
    // `logging` enables the standard `notifications/message` server→client push
    // the subscription rides on (ruling 22, half b); `tools` is the action half.
    { capabilities: { tools: {}, logging: {} } },
  );
  for (const op of operations) {
    server.registerTool(
      op.mcpName,
      {
        title: op.title,
        description: op.description,
        inputSchema: op.argSchema,
        annotations: { readOnlyHint: !op.mutates },
      },
      (args) =>
        runAsTool(() => op.handler(conn, args as Record<string, unknown>)),
    );
  }
  return server;
}

/** Parse the resume cursor env (`POSTHASTE_MCP_AFTER_SEQ`) into a seq, or none. */
function parseAfterSeq(raw: string | undefined): number | undefined {
  if (!raw || raw.trim().length === 0) return undefined;
  const seq = Number(raw);
  return Number.isFinite(seq) && seq >= 0 ? seq : undefined;
}

async function main(): Promise<void> {
  let discovered: Connection;
  try {
    discovered = resolveConnection();
  } catch (error) {
    const message =
      error instanceof ConnectionError ? error.message : String(error);
    process.stderr.write(`posthaste-mcp: ${message}\n`);
    process.exit(1);
  }

  process.stderr.write(
    `posthaste-mcp: connected to ${discovered.baseUrl} (via ${discovered.source}, ` +
      `${discovered.token ? "with token" : "no token"})\n`,
  );

  // Connect-time, per-connection mint: attenuate the discovered bootstrap into a
  // token scoped to exactly the declared grants (default read-only + subscribe;
  // write verbs are an explicit opt-in). Non-fatal on failure (see connect.ts).
  const mint = await mintConnectionToken(discovered, {
    grants: process.env.POSTHASTE_MCP_GRANTS,
    expiry: process.env.POSTHASTE_MCP_TOKEN_EXPIRY,
    account: process.env.POSTHASTE_MCP_ACCOUNT,
  });
  const conn = mint.conn;
  process.stderr.write(`posthaste-mcp: ${mint.detail}\n`);

  const server = buildServer(conn);
  const transport = new StdioServerTransport();
  await server.connect(transport);

  // Half (b): open the event tap and push each fact to the agent as an MCP
  // `notifications/message`. Runs for the life of the connection; a failure ends
  // the subscription (the tools stay usable) rather than tearing down the server.
  const afterSeq = parseAfterSeq(process.env.POSTHASTE_MCP_AFTER_SEQ);
  void subscribeEvents(
    conn,
    { afterSeq },
    {
      fetch: conn.fetch ?? fetch,
      send: (n) =>
        server.server.sendLoggingMessage({
          level: n.level,
          logger: n.logger,
          data: n.data,
        }),
      log: (line) => process.stderr.write(`posthaste-mcp: ${line}\n`),
    },
  ).catch((error) => {
    process.stderr.write(
      `posthaste-mcp: event subscription ended: ${String(error)}\n`,
    );
  });
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
