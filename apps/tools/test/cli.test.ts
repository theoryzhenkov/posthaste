import { describe, expect, test } from "bun:test";

import type { Connection } from "../src/core/connection.js";
import { ConnectionError } from "../src/core/connection.js";
import { operations } from "../src/operations/index.js";
import { run, ExitCode, type RunDeps } from "../src/cli/run.js";
import { fakeFetch, type QueuedResponse, type RecordedRequest } from "./helpers.js";

/** Build RunDeps over a fake fetch, capturing stdout/stderr. */
function makeDeps(responses: QueuedResponse[]): {
  deps: RunDeps;
  out: string[];
  err: string[];
  requests: RecordedRequest[];
} {
  const { fetch, requests } = fakeFetch(responses);
  const out: string[] = [];
  const err: string[] = [];
  const conn: Connection = {
    baseUrl: "http://127.0.0.1:9",
    token: "test-token",
    source: "env",
  };
  const deps: RunDeps = {
    operations,
    resolveConnection: (overrides) =>
      overrides.baseUrl ? { ...conn, baseUrl: overrides.baseUrl, source: "flag" } : conn,
    stdout: (t) => out.push(t),
    stderr: (t) => err.push(t),
    isTty: false,
    env: {},
    readStdin: async () => "",
    readFile: async () => {
      throw new Error("no file");
    },
    fetch,
    version: "test",
  };
  return { deps, out, err, requests };
}

describe("posthastectl dispatch", () => {
  test("no args prints top-level help listing reads, writes, and streaming", async () => {
    const { deps, out } = makeDeps([]);
    const code = await run([], deps);
    expect(code).toBe(ExitCode.Ok);
    const help = out.join("");
    expect(help).toContain("messages search");
    expect(help).toContain("Write commands");
    expect(help).toContain("watch");
    expect(help).toContain("mcp");
    // The auth story is env/file, never a flag.
    expect(help).not.toContain("--token");
  });

  test("unknown command exits 2", async () => {
    const { deps, err } = makeDeps([]);
    expect(await run(["frobnicate"], deps)).toBe(ExitCode.Usage);
    expect(err.join("")).toContain("unknown command");
  });

  test("--help on a command shows schema-derived flags", async () => {
    const { deps, out } = makeDeps([]);
    expect(await run(["messages", "search", "--help"], deps)).toBe(ExitCode.Ok);
    const help = out.join("");
    expect(help).toContain("Usage: posthastectl messages search <query>");
    expect(help).toContain("--limit");
    expect(help).toContain("--cursor");
  });

  test("a read op posts /query and prints compact JSON when piped", async () => {
    const { deps, out, requests } = makeDeps([
      { status: 200, json: { generation: 3, data: { rows: [] } } },
    ]);
    const code = await run(["accounts", "list"], deps);
    expect(code).toBe(ExitCode.Ok);
    expect(requests[0]?.url).toBe("http://127.0.0.1:9/query");
    expect(requests[0]?.body).toEqual({ accounts: {} });
    expect(out.join("")).toBe('{"generation":3,"data":{"rows":[]}}\n');
  });

  test("positional + kebab flags parse into the schema's camelCase args", async () => {
    const { deps, requests } = makeDeps([
      { status: 200, json: { generation: 1, data: { rows: [], nextCursor: null } } },
    ]);
    const code = await run(
      ["messages", "search", "hello world", "--account-id", "a1", "--limit", "5"],
      deps,
    );
    expect(code).toBe(ExitCode.Ok);
    const body = requests[0]?.body as { mailList: Record<string, unknown> };
    expect(body.mailList.freeText).toBe("hello world");
    expect(body.mailList.accountId).toBe("a1");
    expect(body.mailList.limit).toBe(5);
  });

  test("a write op mints an id; --id retries verbatim", async () => {
    const { deps, requests } = makeDeps([
      { status: 200, json: { generation: 9 } },
      { status: 200, json: { generation: 9 } },
    ]);
    expect(
      await run(["sync", "a1"], deps),
    ).toBe(ExitCode.Ok);
    expect(
      await run(["sync", "a1", "--id", "SAME-ID"], deps),
    ).toBe(ExitCode.Ok);
    const first = requests[0]?.body as { id: string };
    const second = requests[1]?.body as { id: string };
    expect(first.id).toMatch(/^[0-9A-HJKMNP-TV-Z]{26}$/);
    expect(second.id).toBe("SAME-ID");
  });

  test("--input seeds args and explicit flags win", async () => {
    const { deps, requests } = makeDeps([{ status: 200, json: { generation: 2 } }]);
    const code = await run(
      [
        "--input",
        JSON.stringify({ accountId: "a1", messageId: "m1", add: ["x"] }),
        "tag",
        "--message-id",
        "m2",
      ],
      deps,
    );
    expect(code).toBe(ExitCode.Ok);
    const body = requests[0]?.body as {
      command: { setKeywords: { messageId: string; change: { add: string[] } } };
    };
    expect(body.command.setKeywords.messageId).toBe("m2");
    expect(body.command.setKeywords.change.add).toEqual(["x"]);
  });

  test("missing required args is a usage error (exit 2), before any request", async () => {
    const { deps, requests, err } = makeDeps([]);
    expect(await run(["messages", "get"], deps)).toBe(ExitCode.Usage);
    expect(requests).toHaveLength(0);
    expect(err.join("")).toContain("accountId");
  });

  test("a typed API error exits 4 with the envelope surfaced", async () => {
    const { deps, err } = makeDeps([
      {
        status: 409,
        json: { kind: "conflict", message: "mailbox not empty", retryable: false },
      },
    ]);
    expect(
      await run(["mailboxes", "delete", "--account-id", "a1", "--mailbox-id", "mb1"], deps),
    ).toBe(ExitCode.Api);
    const text = err.join("");
    expect(text).toContain("[conflict]");
    expect(text).toContain("mailbox not empty");
  });

  test("a connection failure exits 3", async () => {
    const { deps } = makeDeps([]);
    deps.resolveConnection = () => {
      throw new ConnectionError("no backend");
    };
    expect(await run(["accounts", "list"], deps)).toBe(ExitCode.Connection);
  });

  test("--pretty renders indented JSON", async () => {
    const { deps, out } = makeDeps([{ status: 200, json: { generation: 1, data: {} } }]);
    await run(["--pretty", "accounts", "list"], deps);
    expect(out.join("")).toContain('\n  "generation": 1');
  });

  test("events --help documents the no-replay contract", async () => {
    const { deps, out } = makeDeps([]);
    expect(await run(["events", "--help"], deps)).toBe(ExitCode.Ok);
    expect(out.join("")).toContain("NO replay");
  });

  test("watch with an unknown flag is a usage error", async () => {
    const { deps } = makeDeps([]);
    expect(await run(["watch", "--after-seq", "3"], deps)).toBe(ExitCode.Usage);
  });
});
