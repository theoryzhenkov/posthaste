#!/usr/bin/env bun
import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { StdioServerTransport } from "@modelcontextprotocol/sdk/server/stdio.js";

import {
  ApiError,
  ConnectionError,
  resolveConnection,
  type Connection,
} from "./client.js";
import { operations } from "./operations/index.js";

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
    { capabilities: { tools: {} } },
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
