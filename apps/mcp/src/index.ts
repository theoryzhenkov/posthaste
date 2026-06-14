#!/usr/bin/env bun
import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { StdioServerTransport } from "@modelcontextprotocol/sdk/server/stdio.js";

import {
  ConnectionError,
  resolveConnection,
  type Connection,
} from "./client.js";
import { registerCommandTools } from "./tools/commands.js";
import { registerReadTools } from "./tools/read.js";
import { wrapTool } from "./tools/wrap.js";

/**
 * Build the MCP server and register the tool set. Each tool maps 1:1 to one
 * documented `/v1` operation; the daemon does the work, this is a thin adapter.
 */
function buildServer(conn: Connection): McpServer {
  const server = new McpServer(
    { name: "posthaste-mcp", version: "0.0.0" },
    { capabilities: { tools: {} } },
  );
  registerReadTools(server, conn, wrapTool);
  registerCommandTools(server, conn, wrapTool);
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
