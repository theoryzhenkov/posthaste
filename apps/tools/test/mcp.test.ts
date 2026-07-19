import { describe, expect, test } from "bun:test";

import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { InMemoryTransport } from "@modelcontextprotocol/sdk/inMemory.js";

import { operations } from "../src/operations/index.js";
import { buildServer, runAsTool } from "../src/mcp/server.js";
import { ApiCallError } from "../src/core/errors.js";
import { fakeConnection, fakeFetch, type QueuedResponse } from "./helpers.js";

/** Spin up the server over an in-memory transport with a fake backend. */
async function connectedClient(responses: QueuedResponse[]) {
  const { fetch, requests } = fakeFetch(responses);
  const server = buildServer(fakeConnection(fetch), operations);
  const client = new Client({ name: "test-client", version: "0.0.0" });
  const [clientTransport, serverTransport] = InMemoryTransport.createLinkedPair();
  await Promise.all([
    server.connect(serverTransport),
    client.connect(clientTransport),
  ]);
  return { client, requests };
}

describe("MCP server", () => {
  test("registers every operation with readOnlyHint from mutates", async () => {
    const { client } = await connectedClient([]);
    const listed = await client.listTools();
    const byName = new Map(listed.tools.map((t) => [t.name, t]));
    expect(byName.size).toBe(operations.length);
    for (const op of operations) {
      const tool = byName.get(op.mcpName);
      expect(tool).toBeDefined();
      expect(tool?.annotations?.readOnlyHint).toBe(!op.mutates);
      expect(tool?.description).toBe(op.description);
    }
    // Input schemas are derived from the shared zod shapes.
    const search = byName.get("search_messages");
    const props = (search?.inputSchema as { properties?: Record<string, unknown> })
      .properties;
    expect(Object.keys(props ?? {})).toContain("query");
    expect(Object.keys(props ?? {})).toContain("cursor");
  });

  test("a read tool call flows through to POST /query and returns JSON text", async () => {
    const { client, requests } = await connectedClient([
      { status: 200, json: { generation: 2, data: { rows: [] } } },
    ]);
    const result = await client.callTool({ name: "list_accounts", arguments: {} });
    expect(requests[0]?.url).toContain("/query");
    expect(requests[0]?.body).toEqual({ accounts: {} });
    const content = result.content as { type: string; text: string }[];
    expect(JSON.parse(content[0]?.text ?? "")).toEqual({
      generation: 2,
      data: { rows: [] },
    });
  });

  test("a write tool call posts a command envelope with a minted id", async () => {
    const { client, requests } = await connectedClient([
      { status: 200, json: { generation: 8 } },
    ]);
    const result = await client.callTool({
      name: "trigger_sync",
      arguments: { accountId: "a1" },
    });
    const body = requests[0]?.body as { id: string; command: unknown };
    expect(requests[0]?.url).toContain("/command");
    expect(body.id).toMatch(/^[0-9A-HJKMNP-TV-Z]{26}$/);
    expect(body.command).toEqual({ syncNow: { accountId: "a1", mode: null } });
    const content = result.content as { type: string; text: string }[];
    expect(JSON.parse(content[0]?.text ?? "")).toEqual({
      id: body.id,
      generation: 8,
    });
  });

  test("a typed API error becomes a tool error carrying kind, not a crash", async () => {
    const { client } = await connectedClient([
      {
        status: 404,
        json: { kind: "unknownId", message: "no such account", retryable: false },
      },
    ]);
    const result = await client.callTool({
      name: "trigger_sync",
      arguments: { accountId: "missing" },
    });
    expect(result.isError).toBe(true);
    const content = result.content as { type: string; text: string }[];
    expect(content[0]?.text).toContain("[unknownId]");
    expect(content[0]?.text).toContain("no such account");
    expect(content[0]?.text).not.toContain("test-token");
  });

  test("runAsTool marks retryable failures", async () => {
    const result = await runAsTool(() => {
      throw new ApiCallError(
        503,
        { kind: "unavailable", message: "syncing", retryable: true },
        "",
      );
    });
    expect(result.isError).toBe(true);
    expect(result.content[0]?.text).toContain("(retryable)");
  });
});
