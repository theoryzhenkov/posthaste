#!/usr/bin/env bun
// The stdio MCP server entry: discover the backend, register the registry as
// tools, serve stdin/stdout. Reached directly or via `posthastectl mcp`, so
// the one compiled sidecar serves agent hosts with no repo checkout and no
// bun install.

import { StdioServerTransport } from "@modelcontextprotocol/sdk/server/stdio.js";

import { resolveConnection, ConnectionError } from "./core/connection.js";
import { operations } from "./operations/index.js";
import { buildServer } from "./mcp/server.js";

export async function main(): Promise<void> {
  let conn;
  try {
    conn = resolveConnection();
  } catch (error) {
    const message = error instanceof ConnectionError ? error.message : String(error);
    process.stderr.write(`posthaste-mcp: ${message}\n`);
    process.exit(1);
  }

  // Diagnostics carry the origin and source only — never the token.
  process.stderr.write(
    `posthaste-mcp: connected to ${conn.baseUrl} (via ${conn.source})\n`,
  );
  // The session secret grants everything; capability scoping is designed but
  // not yet implemented, so the agent host must be treated as fully trusted.
  process.stderr.write(
    "posthaste-mcp: warning: this connection grants the FULL mail surface (read, write, send, every account) — capability scoping is not implemented yet; treat the agent host as fully trusted\n",
  );

  const server = buildServer(conn, operations);
  const transport = new StdioServerTransport();
  await server.connect(transport);
}

// Only start the transport loop when run as the entry module, so importing
// this module (e.g. from a smoke test) does not block.
if (import.meta.main) {
  main().catch((error) => {
    process.stderr.write(`posthaste-mcp: fatal: ${String(error)}\n`);
    process.exit(1);
  });
}
