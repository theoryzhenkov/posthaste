// The MCP rendering of the shared registry: every operation becomes one
// stdio tool. Read tools carry readOnlyHint; write tools are named and
// described as writes — the agent host owns confirmation policy. Input
// schemas are the operations' zod shapes, which are themselves thin views of
// the generated wire types.

import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";

import type { Connection } from "../core/connection.js";
import { ConnectionError } from "../core/connection.js";
import { ApiCallError, TransportError } from "../core/errors.js";
import type { Operation } from "../operations/index.js";

/** Server identity, shared with `posthastectl --version`. */
export const MCP_SERVER_INFO = { name: "posthaste-tools", version: "0.1.0" };

/**
 * Render an operation's result (or error) as an MCP tool result: JSON text
 * on success, the typed error envelope's message (kind + retryability, never
 * the token) as a tool error on failure.
 */
export async function runAsTool(
  fn: () => Promise<unknown>,
): Promise<{ content: { type: "text"; text: string }[]; isError?: true }> {
  try {
    const result = await fn();
    return {
      content: [{ type: "text", text: JSON.stringify(result ?? null, null, 2) }],
    };
  } catch (error) {
    let message: string;
    if (error instanceof ApiCallError) {
      message = error.retryable ? `${error.message} (retryable)` : error.message;
    } else if (
      error instanceof ConnectionError ||
      error instanceof TransportError ||
      error instanceof Error
    ) {
      message = error.message;
    } else {
      message = String(error);
    }
    return { isError: true, content: [{ type: "text", text: message }] };
  }
}

/**
 * Build the MCP server and register the operation registry as tools. Each
 * tool maps 1:1 to one query family or command intent; the backend does the
 * work, this is a thin adapter.
 */
export function buildServer(conn: Connection, operations: Operation[]): McpServer {
  const server = new McpServer(MCP_SERVER_INFO, { capabilities: { tools: {} } });
  for (const op of operations) {
    server.registerTool(
      op.mcpName,
      {
        title: op.title,
        description: op.description,
        inputSchema: op.argSchema,
        annotations: { readOnlyHint: !op.mutates },
      },
      (args) => runAsTool(() => op.handler(conn, args as Record<string, unknown>)),
    );
  }
  return server;
}
