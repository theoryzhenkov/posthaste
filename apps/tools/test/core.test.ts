import { describe, expect, test } from "bun:test";

import {
  connectionInfoPath,
  resolveConnection,
  ConnectionError,
} from "../src/core/connection.js";
import { mintCommandId, runCommand, runQuery, fetchBlob } from "../src/core/client.js";
import { ApiCallError, TransportError } from "../src/core/errors.js";
import { fakeConnection, fakeFetch } from "./helpers.js";

describe("connection discovery", () => {
  test("env POSTHASTE_API_URL wins over the connection-info file", () => {
    const conn = resolveConnection(
      {},
      {
        env: {
          POSTHASTE_API_URL: "http://127.0.0.1:4000/",
          POSTHASTE_TOKEN: "envtok",
          POSTHASTE_STATE_ROOT: "/nope",
        },
        readFile: () => {
          throw new Error("should not be read");
        },
      },
    );
    expect(conn.baseUrl).toBe("http://127.0.0.1:4000");
    expect(conn.token).toBe("envtok");
    expect(conn.source).toBe("env");
  });

  test("falls back to connection-info.json under the state root", () => {
    const reads: string[] = [];
    const conn = resolveConnection(
      {},
      {
        env: { POSTHASTE_STATE_ROOT: "/state" },
        readFile: (path) => {
          reads.push(path);
          return JSON.stringify({ port: 4321, token: "filetok" });
        },
      },
    );
    expect(reads).toEqual(["/state/connection-info.json"]);
    expect(conn.baseUrl).toBe("http://127.0.0.1:4321");
    expect(conn.token).toBe("filetok");
    expect(conn.source).toBe("connection-info");
  });

  test("state root precedence: POSTHASTE_STATE_ROOT > XDG_DATA_HOME", () => {
    expect(connectionInfoPath({ POSTHASTE_STATE_ROOT: "/a" })).toBe(
      "/a/connection-info.json",
    );
    expect(connectionInfoPath({ XDG_DATA_HOME: "/xdg" })).toBe(
      "/xdg/posthaste/connection-info.json",
    );
  });

  test("a --base-url override still takes the token from env, never argv", () => {
    const conn = resolveConnection(
      { baseUrl: "http://127.0.0.1:5000/" },
      { env: { POSTHASTE_TOKEN: "envtok" }, readFile: () => "" },
    );
    expect(conn.baseUrl).toBe("http://127.0.0.1:5000");
    expect(conn.token).toBe("envtok");
    expect(conn.source).toBe("flag");
  });

  test("no env and no file is an actionable ConnectionError", () => {
    expect(() =>
      resolveConnection(
        {},
        {
          env: { POSTHASTE_STATE_ROOT: "/state" },
          readFile: () => {
            throw new Error("ENOENT");
          },
        },
      ),
    ).toThrow(ConnectionError);
  });

  test("a malformed connection-info file is a ConnectionError", () => {
    expect(() =>
      resolveConnection(
        {},
        {
          env: { POSTHASTE_STATE_ROOT: "/state" },
          readFile: () => JSON.stringify({ port: "not-a-number" }),
        },
      ),
    ).toThrow(ConnectionError);
  });
});

describe("query client", () => {
  test("posts the typed query with bearer auth and returns the envelope", async () => {
    const { fetch, requests } = fakeFetch([
      { status: 200, json: { generation: 7, data: { rows: [] } } },
    ]);
    const conn = fakeConnection(fetch);
    const answer = await runQuery(conn, { accounts: {} });
    expect(answer.generation).toBe(7);
    expect(requests[0]?.url).toBe("http://127.0.0.1:9/query");
    expect(requests[0]?.method).toBe("POST");
    expect(requests[0]?.headers.authorization).toBe("Bearer test-token");
    expect(requests[0]?.body).toEqual({ accounts: {} });
  });

  test("a typed error envelope surfaces kind and retryability", async () => {
    const { fetch } = fakeFetch([
      {
        status: 404,
        json: { kind: "unknownId", message: "no such message", retryable: false },
      },
    ]);
    const conn = fakeConnection(fetch);
    try {
      await runQuery(conn, { accounts: {} });
      expect.unreachable();
    } catch (error) {
      expect(error).toBeInstanceOf(ApiCallError);
      const api = error as ApiCallError;
      expect(api.kind).toBe("unknownId");
      expect(api.retryable).toBe(false);
      expect(api.status).toBe(404);
      expect(api.message).toContain("no such message");
      // The token never leaks into error text.
      expect(api.message).not.toContain("test-token");
    }
  });

  test("a non-envelope error body still throws with the raw text", async () => {
    const { fetch } = fakeFetch([{ status: 500, text: "boom" }]);
    try {
      await runQuery(fakeConnection(fetch), { accounts: {} });
      expect.unreachable();
    } catch (error) {
      const api = error as ApiCallError;
      expect(api.kind).toBeUndefined();
      expect(api.message).toContain("boom");
    }
  });

  test("a network failure is a TransportError", async () => {
    const failing = (async () => {
      throw new Error("ECONNREFUSED");
    }) as unknown as typeof fetch;
    await expect(
      runQuery(fakeConnection(failing), { accounts: {} }),
    ).rejects.toBeInstanceOf(TransportError);
  });
});

describe("command client", () => {
  test("mints an idempotency id and returns it with the generation", async () => {
    const { fetch, requests } = fakeFetch([{ status: 200, json: { generation: 12 } }]);
    const conn = fakeConnection(fetch);
    const accepted = await runCommand(conn, {
      syncNow: { accountId: "a1", mode: null },
    });
    expect(accepted.generation).toBe(12);
    expect(accepted.id).toMatch(/^[0-9A-HJKMNP-TV-Z]{26}$/);
    const body = requests[0]?.body as { id: string; command: unknown };
    expect(body.id).toBe(accepted.id);
    expect(body.command).toEqual({ syncNow: { accountId: "a1", mode: null } });
  });

  test("a caller-supplied id is sent verbatim (safe retry)", async () => {
    const { fetch, requests } = fakeFetch([{ status: 200, json: { generation: 1 } }]);
    const accepted = await runCommand(
      fakeConnection(fetch),
      { syncNow: { accountId: "a1", mode: null } },
      "RETRY-ID",
    );
    expect(accepted.id).toBe("RETRY-ID");
    expect((requests[0]?.body as { id: string }).id).toBe("RETRY-ID");
  });

  test("mintCommandId is 26 chars, unique, and time-ordered", () => {
    const a = mintCommandId(1_000_000);
    const b = mintCommandId(2_000_000);
    expect(a).toHaveLength(26);
    expect(a.slice(0, 10) < b.slice(0, 10)).toBe(true);
    expect(mintCommandId()).not.toBe(mintCommandId());
  });
});

describe("blob client", () => {
  test("fetches bytes with auth and encodes the id", async () => {
    const { fetch, requests } = fakeFetch([{ status: 200, text: "bytes" }]);
    const bytes = await fetchBlob(fakeConnection(fetch), "b/1");
    expect(new TextDecoder().decode(bytes)).toBe("bytes");
    expect(requests[0]?.url).toBe("http://127.0.0.1:9/blobs/b%2F1");
  });
});
