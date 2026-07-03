import { describe, expect, test } from "bun:test";

import type { Connection } from "../src/client.js";
import {
  DEFAULT_MCP_GRANTS,
  mintConnectionToken,
  resolveConnectGrants,
} from "../src/connect.js";

interface Call {
  url: string;
  method: string;
  body: { actions?: string[]; expiresInSeconds?: number; account?: string };
  auth?: string;
}

/** A connection whose fetch returns a fixed minted token and records the call. */
function harness(): { conn: Connection; calls: Call[] } {
  const calls: Call[] = [];
  const fetch: typeof globalThis.fetch = async (input, init) => {
    const headers = new Headers(init?.headers);
    calls.push({
      url: String(input),
      method: init?.method ?? "GET",
      body: init?.body ? JSON.parse(String(init.body)) : {},
      auth: headers.get("authorization") ?? undefined,
    });
    return new Response(
      JSON.stringify({
        token: "minted-scoped",
        expiresAt: "2099-01-01T00:00:00Z",
      }),
      { headers: { "content-type": "application/json" } },
    );
  };
  const conn: Connection = {
    baseUrl: "http://daemon/v1",
    token: "bootstrap-full",
    source: "daemon.json",
    fetch,
  };
  return { conn, calls };
}

describe("resolveConnectGrants — least-grant default", () => {
  test("no declaration defaults to read-only + subscribe", () => {
    expect(resolveConnectGrants(undefined)).toEqual(["read"]);
    expect(resolveConnectGrants("")).toEqual(["read"]);
    expect(DEFAULT_MCP_GRANTS).toBe("tap:read,read");
  });

  test("an explicit apply opt-in widens to the write verbs", () => {
    expect(new Set(resolveConnectGrants("read,apply"))).toEqual(
      new Set(["read", "tag", "move", "delete"]),
    );
  });
});

describe("mintConnectionToken — connect-time mint", () => {
  test("mints a scoped token and runs the connection under it", async () => {
    const { conn, calls } = harness();
    const result = await mintConnectionToken(conn, {
      grants: "read,apply",
      expiry: "1h",
      account: "acct-a",
    });

    expect(result.minted).toBe(true);
    expect(result.conn.token).toBe("minted-scoped");
    // The bootstrap token authenticates the attenuation.
    expect(calls).toHaveLength(1);
    expect(calls[0]!.url).toBe("http://daemon/v1/auth/tokens");
    expect(calls[0]!.method).toBe("POST");
    expect(calls[0]!.auth).toBe("Bearer bootstrap-full");
    expect(new Set(calls[0]!.body.actions)).toEqual(
      new Set(["read", "tag", "move", "delete"]),
    );
    expect(calls[0]!.body.expiresInSeconds).toBe(3600);
    expect(calls[0]!.body.account).toBe("acct-a");
  });

  test("defaults to read-only + subscribe when no grants are declared", async () => {
    const { conn, calls } = harness();
    const result = await mintConnectionToken(conn, {});
    expect(result.actions).toEqual(["read"]);
    expect(calls[0]!.body.actions).toEqual(["read"]);
  });

  test("an auth-disabled daemon (no bootstrap token) skips minting", async () => {
    const { conn, calls } = harness();
    const result = await mintConnectionToken(
      { ...conn, token: undefined },
      { grants: "read" },
    );
    expect(result.minted).toBe(false);
    expect(result.conn.token).toBeUndefined();
    expect(calls).toHaveLength(0);
  });

  test("a mint failure is non-fatal — falls back to the bootstrap token", async () => {
    const failing: Connection = {
      baseUrl: "http://daemon/v1",
      token: "bootstrap-full",
      source: "daemon.json",
      fetch: async () =>
        new Response(JSON.stringify({ code: "forbidden", message: "nope" }), {
          status: 403,
          headers: { "content-type": "application/json" },
        }),
    };
    const result = await mintConnectionToken(failing, { grants: "read" });
    expect(result.minted).toBe(false);
    expect(result.conn.token).toBe("bootstrap-full");
    expect(result.detail).toContain("mint failed");
  });
});
