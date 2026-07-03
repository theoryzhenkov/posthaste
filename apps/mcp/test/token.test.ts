import { describe, expect, test } from "bun:test";

import type { Connection, ConnectionOverrides } from "../src/client.js";
import { operations } from "../src/operations/index.js";
import { ExitCode, run, type RunDeps } from "../src/cli/run.js";
import { parseDuration, parseTokenMintOptions } from "../src/cli/token.js";

interface Capture {
  out: string;
  err: string;
  calls: { url: string; method: string; body: unknown; auth?: string }[];
}

/** A `RunDeps` bag whose fetch stub returns a fixed minted token. */
function harness(connection?: (o: ConnectionOverrides) => Connection): {
  deps: RunDeps;
  cap: Capture;
} {
  const cap: Capture = { out: "", err: "", calls: [] };
  const stubFetch: typeof fetch = async (input, init) => {
    const url = String(input);
    const headers = new Headers(init?.headers);
    cap.calls.push({
      url,
      method: init?.method ?? "GET",
      body: init?.body ? JSON.parse(String(init.body)) : undefined,
      auth: headers.get("authorization") ?? undefined,
    });
    return new Response(
      JSON.stringify({
        token: "minted-macaroon",
        expiresAt: "2099-01-01T00:00:00Z",
      }),
      { headers: { "content-type": "application/json" } },
    );
  };
  const deps: RunDeps = {
    operations,
    resolveConnection:
      connection ??
      ((o) => ({
        baseUrl: o.baseUrl ?? "http://daemon/v1",
        token: o.token ?? "bootstrap-full-scope",
        source: "daemon.json",
      })),
    stdout: (t) => (cap.out += t),
    stderr: (t) => (cap.err += t),
    isTty: false,
    env: {},
    readStdin: async () => "",
    readFile: async () => "",
    writeFile: async () => {},
    fetch: stubFetch,
    version: "9.9.9",
  };
  return { deps, cap };
}

describe("token mint — scope mapping", () => {
  test("tap:read / read / apply map to the wire action verbs (deduped)", () => {
    const opts = parseTokenMintOptions(["--grant", "tap:read,apply,read"]);
    // tap:read → read, read → read (deduped), apply → tag,move,delete
    expect(new Set(opts.actions)).toEqual(
      new Set(["read", "tag", "move", "delete"]),
    );
  });

  test("raw verbs pass through; unknown grants are a usage error", () => {
    expect(parseTokenMintOptions(["--grant", "send"]).actions).toEqual([
      "send",
    ]);
    expect(() => parseTokenMintOptions(["--grant", "bogus"])).toThrow(
      /unknown grant/,
    );
  });

  test("at least one grant is required", () => {
    expect(() => parseTokenMintOptions([])).toThrow(
      /requires at least one --grant/,
    );
  });

  test("--expiry parses human durations to seconds", () => {
    expect(parseDuration("3600")).toBe(3600);
    expect(parseDuration("90m")).toBe(5400);
    expect(parseDuration("1h")).toBe(3600);
    expect(parseDuration("7d")).toBe(604800);
    expect(() => parseDuration("0")).toThrow();
    expect(() => parseDuration("nope")).toThrow();
  });
});

describe("token mint — end to end via run()", () => {
  test("POSTs the attenuation request and prints the bare token to stdout", async () => {
    const { deps, cap } = harness();
    const code = await run(
      [
        "token",
        "mint",
        "--grant",
        "tap:read,apply,read",
        "--expiry",
        "1h",
        "--account",
        "acct-a",
      ],
      deps,
    );
    expect(code).toBe(ExitCode.Ok);
    expect(cap.calls).toHaveLength(1);
    const call = cap.calls[0]!;
    expect(call.method).toBe("POST");
    expect(call.url).toBe("http://daemon/v1/auth/tokens");
    // The bootstrap token authenticates the mint (attenuation is caller-scoped).
    expect(call.auth).toBe("Bearer bootstrap-full-scope");
    const body = call.body as {
      actions: string[];
      expiresInSeconds: number;
      account: string;
    };
    expect(new Set(body.actions)).toEqual(
      new Set(["read", "tag", "move", "delete"]),
    );
    expect(body.expiresInSeconds).toBe(3600);
    expect(body.account).toBe("acct-a");
    // stdout is exactly the credential (so TOKEN=$(...) captures only it).
    expect(cap.out.trim()).toBe("minted-macaroon");
    // The ready-to-paste line + scope summary go to stderr.
    expect(cap.err).toContain("export POSTHASTE_TOKEN=minted-macaroon");
    expect(cap.err).toContain("expires 2099-01-01T00:00:00Z");
  });

  test("--help prints usage without a network call", async () => {
    const { deps, cap } = harness();
    expect(await run(["token", "--help"], deps)).toBe(ExitCode.Ok);
    expect(cap.out).toContain("Usage: posthastectl token mint");
    expect(cap.calls).toHaveLength(0);
  });

  test("an unknown token subcommand is a usage error", async () => {
    const { deps } = harness();
    expect(await run(["token", "bogus"], deps)).toBe(ExitCode.Usage);
  });
});
