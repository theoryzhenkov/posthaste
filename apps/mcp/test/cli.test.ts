import { describe, expect, test } from "bun:test";

import type { Connection, ConnectionOverrides } from "../src/client.js";
import { operations } from "../src/operations/index.js";
import { ExitCode, run, type RunDeps } from "../src/cli/run.js";

interface Capture {
  out: string;
  err: string;
  calls: { url: string; method: string; body: unknown; auth?: string }[];
}

interface Harness {
  deps: RunDeps;
  cap: Capture;
}

/** Build `RunDeps` with capture buffers and a programmable fetch stub. */
function harness(
  opts: {
    fetch?: typeof fetch;
    stdin?: string;
    file?: string;
    isTty?: boolean;
    connection?: (o: ConnectionOverrides) => Connection;
  } = {},
): Harness {
  const cap: Capture = { out: "", err: "", calls: [] };
  const defaultFetch: typeof fetch = async (input, init) => {
    const url = String(input);
    const headers = new Headers(init?.headers);
    cap.calls.push({
      url,
      method: init?.method ?? "GET",
      body: init?.body ? JSON.parse(String(init.body)) : undefined,
      auth: headers.get("authorization") ?? undefined,
    });
    return new Response(JSON.stringify({ ok: true, url }), {
      headers: { "content-type": "application/json" },
    });
  };
  const deps: RunDeps = {
    operations,
    resolveConnection:
      opts.connection ??
      ((o) => ({
        baseUrl: o.baseUrl ?? "http://daemon/v1",
        token: o.token,
        source: "flag",
      })),
    stdout: (t) => (cap.out += t),
    stderr: (t) => (cap.err += t),
    isTty: opts.isTty ?? false,
    env: {},
    readStdin: async () => opts.stdin ?? "",
    readFile: async () => opts.file ?? "",
    writeFile: async () => {},
    fetch: opts.fetch ?? defaultFetch,
    version: "9.9.9",
  };
  return { deps, cap };
}

describe("run — help and version", () => {
  test("no args prints top-level help, exit 0", async () => {
    const { deps, cap } = harness();
    expect(await run([], deps)).toBe(ExitCode.Ok);
    expect(cap.out).toContain("Usage: posthastectl");
    expect(cap.out).toContain("messages search");
  });

  test("--version prints version, exit 0", async () => {
    const { deps, cap } = harness();
    expect(await run(["--version"], deps)).toBe(ExitCode.Ok);
    expect(cap.out.trim()).toBe("posthastectl 9.9.9");
  });

  test("command --help prints command usage, exit 0, no fetch", async () => {
    const { deps, cap } = harness();
    expect(await run(["messages", "search", "--help"], deps)).toBe(ExitCode.Ok);
    expect(cap.out).toContain("Usage: posthastectl messages search");
    expect(cap.calls).toHaveLength(0);
  });
});

describe("run — command dispatch and I/O", () => {
  test("positional fills the primary field and hits the right URL", async () => {
    const { deps, cap } = harness();
    const code = await run(
      ["messages", "search", "hello", "--limit", "5"],
      deps,
    );
    expect(code).toBe(ExitCode.Ok);
    expect(cap.calls).toHaveLength(1);
    const call = cap.calls[0]!;
    expect(call.method).toBe("GET");
    expect(call.url).toBe("http://daemon/v1/messages/search?q=hello&limit=5");
  });

  test("two-segment command + path-param interpolation", async () => {
    const { deps, cap } = harness();
    const code = await run(
      ["messages", "get", "--source-id", "acct1", "--message-id", "m9"],
      deps,
    );
    expect(code).toBe(ExitCode.Ok);
    expect(cap.calls[0]!.url).toBe(
      "http://daemon/v1/sources/acct1/messages/m9",
    );
  });

  test("--input seeds args, explicit flags override", async () => {
    const { deps, cap } = harness({
      stdin: '{"sourceId":"a","messageId":"m","add":["X"],"remove":[]}',
    });
    const code = await run(
      ["messages", "set-keywords", "-i", "-", "--message-id", "OVERRIDE"],
      deps,
    );
    expect(code).toBe(ExitCode.Ok);
    const call = cap.calls[0]!;
    expect(call.url).toContain("/messages/OVERRIDE/set-keywords");
    expect(call.body).toEqual({ add: ["X"], remove: [] });
  });

  test("repeated array flags collect into an array", async () => {
    const { deps, cap } = harness();
    const code = await run(
      [
        "messages",
        "set-keywords",
        "--source-id",
        "a",
        "--message-id",
        "m",
        "--add",
        "\\Seen",
        "--add",
        "\\Flagged",
        "--remove",
        "[]",
      ],
      deps,
    );
    expect(code).toBe(ExitCode.Ok);
    expect(cap.calls[0]!.body).toEqual({
      add: ["\\Seen", "\\Flagged"],
      remove: [],
    });
  });

  test("token override is sent as a bearer header", async () => {
    const { deps, cap } = harness();
    await run(["accounts", "list", "--token", "sekret"], deps);
    expect(cap.calls[0]!.auth).toBe("Bearer sekret");
  });

  test("compact output by default when piped; --pretty indents", async () => {
    const compact = harness();
    await run(["accounts", "list"], compact.deps);
    expect(compact.cap.out).not.toContain("\n  ");

    const pretty = harness();
    await run(["accounts", "list", "--pretty"], pretty.deps);
    expect(pretty.cap.out).toContain("\n  ");
  });
});

describe("run — error paths and exit codes", () => {
  test("unknown command → exit 2", async () => {
    const { deps, cap } = harness();
    expect(await run(["frobnicate"], deps)).toBe(ExitCode.Usage);
    expect(cap.err).toContain("unknown command");
  });

  test("missing required arg → exit 2, no fetch", async () => {
    const { deps, cap } = harness();
    expect(await run(["messages", "get"], deps)).toBe(ExitCode.Usage);
    expect(cap.calls).toHaveLength(0);
    expect(cap.err).toContain("sourceId");
  });

  test("unknown flag → exit 2", async () => {
    const { deps } = harness();
    expect(await run(["accounts", "list", "--bogus", "x"], deps)).toBe(
      ExitCode.Usage,
    );
  });

  test("API error → exit 4 with code", async () => {
    const fetch: typeof globalThis.fetch = async () =>
      new Response(JSON.stringify({ code: "forbidden", message: "nope" }), {
        status: 403,
        headers: { "content-type": "application/json" },
      });
    const { deps, cap } = harness({ fetch });
    expect(await run(["accounts", "list"], deps)).toBe(ExitCode.Api);
    expect(cap.err).toContain("[forbidden]");
  });

  test("connection error → exit 3", async () => {
    const { deps, cap } = harness({
      connection: () => {
        throw new (require("../src/client.js").ConnectionError)("no daemon");
      },
    });
    expect(await run(["accounts", "list"], deps)).toBe(ExitCode.Connection);
    expect(cap.err).toContain("no daemon");
  });

  test("invalid --input JSON → exit 2", async () => {
    const { deps, cap } = harness({ stdin: "not json" });
    expect(await run(["accounts", "list", "-i", "-"], deps)).toBe(
      ExitCode.Usage,
    );
    expect(cap.err).toContain("not valid JSON");
  });
});

describe("run — events tap", () => {
  test("streams SSE data frames as NDJSON, exit 0", async () => {
    const sse =
      'id: 1\ndata: {"seq":1,"topic":"sync.completed"}\n\n' +
      'id: 2\ndata: {"seq":2,"topic":"message.updated"}\n\n';
    const fetch: typeof globalThis.fetch = async () =>
      new Response(sse, { headers: { "content-type": "text/event-stream" } });
    const { deps, cap } = harness({ fetch });
    expect(await run(["events", "--topic", "sync.completed"], deps)).toBe(
      ExitCode.Ok,
    );
    const lines = cap.out.trim().split("\n");
    expect(lines).toHaveLength(2);
    expect(JSON.parse(lines[0]!)).toEqual({ seq: 1, topic: "sync.completed" });
    expect(JSON.parse(lines[1]!)).toEqual({ seq: 2, topic: "message.updated" });
  });

  test("events on a 404 daemon → exit 4 with a hint", async () => {
    const fetch: typeof globalThis.fetch = async () =>
      new Response("", { status: 404 });
    const { deps, cap } = harness({ fetch });
    expect(await run(["events"], deps)).toBe(ExitCode.Api);
    expect(cap.err).toContain("GET /v1/events");
  });
});

describe("run — dispatcher wiring: watch --topic/--rule, hook serve", () => {
  test("top-level help lists 'hook serve'", async () => {
    const { deps, cap } = harness();
    expect(await run([], deps)).toBe(ExitCode.Ok);
    expect(cap.out).toContain("hook serve");
  });

  test("watch --help documents --topic and --rule as convenience filters", async () => {
    const { deps, cap } = harness();
    expect(await run(["watch", "--help"], deps)).toBe(ExitCode.Ok);
    expect(cap.out).toContain("--topic <topic>");
    expect(cap.out).toContain("--rule <name>");
    expect(cap.out).toContain("convenience, not an auth boundary");
  });

  test("watch --rule without --exec/--topic is still accepted (parses cleanly)", async () => {
    const sse =
      'id: 1\ndata: {"seq":1,"topic":"rule.fired","accountId":"a","messageId":"m","payload":{"ruleId":"r1"}}\n\n';
    const fetch: typeof globalThis.fetch = async (input) => {
      const url = String(input);
      if (url.includes("/events")) {
        expect(url).toContain("topic=rule.fired");
        return new Response(sse, {
          headers: { "content-type": "text/event-stream" },
        });
      }
      return Response.json({ keywords: [], mailboxIds: [] });
    };
    const { deps } = harness({ fetch });
    expect(
      await run(["watch", "--topic", "rule.fired", "--rule", "r1"], deps),
    ).toBe(ExitCode.Ok);
  });

  test("hook serve --help prints usage, exit 0", async () => {
    const { deps, cap } = harness();
    expect(await run(["hook", "serve", "--help"], deps)).toBe(ExitCode.Ok);
    expect(cap.out).toContain("Usage: posthastectl hook serve");
    expect(cap.out).toContain("payload-is-data");
  });

  test("'hook' with no/unknown subcommand → usage error, exit 2", async () => {
    const { deps, cap } = harness();
    expect(await run(["hook"], deps)).toBe(ExitCode.Usage);
    expect(cap.err).toContain("hook serve");
    expect(await run(["hook", "bogus"], deps)).toBe(ExitCode.Usage);
  });

  test("hook serve without --exec → usage error, exit 2, no server started", async () => {
    const { deps, cap } = harness();
    expect(await run(["hook", "serve"], deps)).toBe(ExitCode.Usage);
    expect(cap.err).toContain("--exec");
  });

  test("hook serve --port not-a-number → usage error, exit 2", async () => {
    const { deps, cap } = harness();
    expect(
      await run(["hook", "serve", "--exec", "./h.sh", "--port", "nope"], deps),
    ).toBe(ExitCode.Usage);
    expect(cap.err).toContain("--port");
  });

  test("hook serve starts, accepts a delivery, and stops cleanly on abort", async () => {
    const controller = new AbortController();
    const { deps, cap } = harness();
    const deliveries: { input: string; env: Record<string, string> }[] = [];
    deps.runCommand = async (_command, input, env) => {
      deliveries.push({ input, env });
      return 0;
    };
    deps.signal = controller.signal;

    const runPromise = run(
      ["hook", "serve", "--exec", "./h.sh", "--port", "0"],
      deps,
    );

    // Wait for the "listening on <url>" diagnostic to land on stderr, then
    // pull the OS-assigned port back out of it.
    const deadline = Date.now() + 2000;
    let url: string | undefined;
    while (Date.now() < deadline) {
      const match = /listening on (http:\/\/127\.0\.0\.1:\d+\/hook)/.exec(
        cap.err,
      );
      if (match) {
        url = match[1];
        break;
      }
      await new Promise((r) => setTimeout(r, 5));
    }
    expect(url).toBeDefined();

    const res = await fetch(url as string, {
      method: "POST",
      body: JSON.stringify({ ruleId: "r1", event: {}, message: {} }),
    });
    expect(res.status).toBe(200);
    expect(deliveries).toHaveLength(1);
    expect(deliveries[0]!.env.PH_RULE).toBe("r1");

    controller.abort();
    expect(await runPromise).toBe(ExitCode.Ok);
  });
});
