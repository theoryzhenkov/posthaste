import { describe, expect, test } from "bun:test";

import type { Connection, ConnectionOverrides } from "../src/client.js";
import { operations } from "../src/operations/index.js";
import { ExitCode, run, type RunDeps } from "../src/cli/run.js";
import {
  deriveIdempotencyKey,
  parseApplyOptions,
  parseMoveOptions,
  parseReplyOptions,
  parseSendOptions,
  parseTagOptions,
  resolveAccount,
  resolveBodyArg,
  resolveMessage,
} from "../src/cli/writeVerbs.js";

interface Call {
  url: string;
  method: string;
  body: unknown;
  auth?: string;
  idempotencyKey?: string;
}

interface Capture {
  out: string;
  err: string;
  calls: Call[];
}

/** A `RunDeps` bag whose fetch stub routes by URL suffix + method. */
function harness(
  opts: {
    routes: Record<string, unknown>;
    env?: Record<string, string | undefined>;
    stdin?: string;
    file?: string;
    connection?: (o: ConnectionOverrides) => Connection;
  } & { routes: Record<string, unknown> },
): { deps: RunDeps; cap: Capture } {
  const cap: Capture = { out: "", err: "", calls: [] };
  const stubFetch: typeof fetch = async (input, init) => {
    const url = String(input);
    const headers = new Headers(init?.headers);
    const method = init?.method ?? "GET";
    cap.calls.push({
      url,
      method,
      body: init?.body ? JSON.parse(String(init.body)) : undefined,
      auth: headers.get("authorization") ?? undefined,
      idempotencyKey: headers.get("idempotency-key") ?? undefined,
    });
    const key = `${method} ${new URL(url).pathname}`;
    const match = opts.routes[key];
    if (match === undefined) {
      throw new Error(`unmocked route: ${key}`);
    }
    return new Response(JSON.stringify(match), {
      headers: { "content-type": "application/json" },
    });
  };
  const deps: RunDeps = {
    operations,
    resolveConnection:
      opts.connection ??
      ((o) => ({
        baseUrl: o.baseUrl ?? "http://daemon/v1",
        token: o.token ?? "the-token",
        source: "daemon.json",
      })),
    stdout: (t) => (cap.out += t),
    stderr: (t) => (cap.err += t),
    isTty: false,
    env: opts.env ?? {},
    readStdin: async () => opts.stdin ?? "",
    readFile: async () => opts.file ?? "",
    writeFile: async () => {},
    fetch: stubFetch,
    version: "9.9.9",
  };
  return { deps, cap };
}

describe("write verbs — pure helpers", () => {
  test("resolveAccount/resolveMessage: flag wins, then PH_ACCOUNT, then PH_ACCOUNT_ID", () => {
    expect(
      resolveAccount("flag", { PH_ACCOUNT: "a", PH_ACCOUNT_ID: "b" }),
    ).toBe("flag");
    expect(
      resolveAccount(undefined, { PH_ACCOUNT: "a", PH_ACCOUNT_ID: "b" }),
    ).toBe("a");
    expect(resolveAccount(undefined, { PH_ACCOUNT_ID: "b" })).toBe("b");
    expect(resolveAccount(undefined, {})).toBeUndefined();
    expect(resolveMessage("flag", { PH_MESSAGE_ID: "m" })).toBe("flag");
    expect(resolveMessage(undefined, { PH_MESSAGE_ID: "m" })).toBe("m");
    expect(resolveMessage(undefined, {})).toBeUndefined();
  });

  test("resolveBodyArg: literal text, '-' reads stdin, '@file' reads a file", async () => {
    const deps = {
      readStdin: async () => "from-stdin",
      readFile: async () => "from-file",
    };
    expect(await resolveBodyArg("hello", deps)).toBe("hello");
    expect(await resolveBodyArg("-", deps)).toBe("from-stdin");
    expect(await resolveBodyArg("@notes.txt", deps)).toBe("from-file");
  });

  describe("deriveIdempotencyKey", () => {
    test("an explicit --idempotency-key always wins", () => {
      const warnings: string[] = [];
      const key = deriveIdempotencyKey(
        "explicit-key",
        "tag",
        { PH_EVENT_SEQ: "5" },
        (l) => warnings.push(l),
      );
      expect(key).toBe("explicit-key");
      expect(warnings).toHaveLength(0);
    });

    test("PH_IDEMPOTENCY_KEY (the rule-exec-derived key) is reused, suffixed by verb", () => {
      const key = deriveIdempotencyKey(
        undefined,
        "tag",
        { PH_IDEMPOTENCY_KEY: "rule:tagger:91" },
        () => {},
      );
      expect(key).toBe("rule:tagger:91:tag");
    });

    test("falls back to PH_EVENT_SEQ, then PH_SEQ, when no PH_IDEMPOTENCY_KEY", () => {
      expect(
        deriveIdempotencyKey(
          undefined,
          "tag",
          { PH_EVENT_SEQ: "42" },
          () => {},
        ),
      ).toBe("seq:42:tag");
      expect(
        deriveIdempotencyKey(undefined, "tag", { PH_SEQ: "42" }, () => {}),
      ).toBe("seq:42:tag");
    });

    test("the SAME PH_EVENT_SEQ always derives the SAME key for a given verb (server-side dedupe gate)", () => {
      const env = { PH_EVENT_SEQ: "77" };
      const a = deriveIdempotencyKey(undefined, "tag", env, () => {});
      const b = deriveIdempotencyKey(undefined, "tag", env, () => {});
      expect(a).toBe(b);
    });

    test("a DIFFERENT PH_EVENT_SEQ derives a different key", () => {
      const a = deriveIdempotencyKey(
        undefined,
        "tag",
        { PH_EVENT_SEQ: "1" },
        () => {},
      );
      const b = deriveIdempotencyKey(
        undefined,
        "tag",
        { PH_EVENT_SEQ: "2" },
        () => {},
      );
      expect(a).not.toBe(b);
    });

    test("two DIFFERENT verbs sharing one event never collide (avoids the server's 409 reuse rule)", () => {
      const env = { PH_EVENT_SEQ: "77" };
      const tagKey = deriveIdempotencyKey(undefined, "tag", env, () => {});
      const moveKey = deriveIdempotencyKey(undefined, "move", env, () => {});
      expect(tagKey).not.toBe(moveKey);
    });

    test("with no event context at all, a fresh key is generated and a warning fires", () => {
      const warnings: string[] = [];
      const a = deriveIdempotencyKey(undefined, "tag", {}, (l) =>
        warnings.push(l),
      );
      const b = deriveIdempotencyKey(undefined, "tag", {}, (l) =>
        warnings.push(l),
      );
      expect(a).not.toBe(b); // not stable across calls — the warning says so
      expect(warnings).toHaveLength(2);
      expect(warnings[0]).toContain("no PH_EVENT_SEQ");
    });
  });
});

describe("write verbs — flag parsing", () => {
  test("tag requires at least one --add or --remove", () => {
    expect(() => parseTagOptions([])).toThrow(/at least one --add or --remove/);
    expect(parseTagOptions(["--add", "x"]).add).toEqual(["x"]);
  });

  test("move requires --to-mailbox", () => {
    expect(() => parseMoveOptions([])).toThrow(/--to-mailbox/);
    expect(parseMoveOptions(["--to-mailbox", "archive"]).toMailbox).toBe(
      "archive",
    );
  });

  test("reply requires --body", () => {
    expect(() => parseReplyOptions([])).toThrow(/--body/);
  });

  test("send requires --to, --subject, and --body", () => {
    expect(() => parseSendOptions(["--subject", "s", "--body", "b"])).toThrow(
      /--to/,
    );
    expect(() => parseSendOptions(["--to", "a@x.com", "--body", "b"])).toThrow(
      /--subject/,
    );
    expect(() =>
      parseSendOptions(["--to", "a@x.com", "--subject", "s"]),
    ).toThrow(/--body/);
  });

  test("apply requires a known --kind, and --body unless --kind destroy", () => {
    expect(() => parseApplyOptions([])).toThrow(/--kind/);
    expect(() => parseApplyOptions(["--kind", "bogus"])).toThrow(
      /unknown --kind/,
    );
    expect(() => parseApplyOptions(["--kind", "set-keywords"])).toThrow(
      /requires --body/,
    );
    expect(parseApplyOptions(["--kind", "destroy"]).kind).toBe("destroy");
  });
});

describe("tag — end to end via run()", () => {
  test("POSTs set-keywords with the typed body and an auto-derived Idempotency-Key", async () => {
    const { deps, cap } = harness({
      routes: {
        "POST /v1/sources/acct/commands/messages/m1/set-keywords": {
          events: [],
        },
      },
      env: { PH_EVENT_SEQ: "91" },
    });
    const code = await run(
      ["tag", "--account", "acct", "--message", "m1", "--add", "reviewed"],
      deps,
    );
    expect(code).toBe(ExitCode.Ok);
    expect(cap.calls).toHaveLength(1);
    const call = cap.calls[0]!;
    expect(call.method).toBe("POST");
    expect(call.body).toEqual({ add: ["reviewed"], remove: [] });
    expect(call.auth).toBe("Bearer the-token");
    expect(call.idempotencyKey).toBe("seq:91:tag");
  });

  test("--account/--message fall back to PH_ACCOUNT/PH_MESSAGE_ID — the 2-line handler form", async () => {
    const { deps, cap } = harness({
      routes: {
        "POST /v1/sources/acct-env/commands/messages/msg-env/set-keywords": {
          events: [],
        },
      },
      env: { PH_ACCOUNT: "acct-env", PH_MESSAGE_ID: "msg-env" },
    });
    const code = await run(["tag", "--add", "reviewed"], deps);
    expect(code).toBe(ExitCode.Ok);
    expect(cap.calls[0]!.url).toContain(
      "/sources/acct-env/commands/messages/msg-env/set-keywords",
    );
  });

  test("missing --account and no env fallback is a usage error", async () => {
    const { deps, cap } = harness({ routes: {} });
    const code = await run(["tag", "--message", "m1", "--add", "x"], deps);
    expect(code).toBe(ExitCode.Usage);
    expect(cap.calls).toHaveLength(0);
    expect(cap.err).toContain("--account is required");
  });

  test("--idempotency-key overrides the derived key", async () => {
    const { deps, cap } = harness({
      routes: {
        "POST /v1/sources/acct/commands/messages/m1/set-keywords": {
          events: [],
        },
      },
      env: { PH_EVENT_SEQ: "91" },
    });
    await run(
      [
        "tag",
        "--account",
        "acct",
        "--message",
        "m1",
        "--add",
        "x",
        "--idempotency-key",
        "custom-key",
      ],
      deps,
    );
    expect(cap.calls[0]!.idempotencyKey).toBe("custom-key");
  });

  test("--help prints usage without a network call", async () => {
    const { deps, cap } = harness({ routes: {} });
    expect(await run(["tag", "--help"], deps)).toBe(ExitCode.Ok);
    expect(cap.out).toContain("Usage: posthastectl tag");
    expect(cap.calls).toHaveLength(0);
  });
});

describe("move — end to end via run()", () => {
  test("resolves a mailbox role, then POSTs replace-mailboxes", async () => {
    const { deps, cap } = harness({
      routes: {
        "GET /v1/sources/acct/mailboxes": [
          {
            id: "mbx-1",
            name: "Inbox",
            role: "inbox",
            totalEmails: 0,
            unreadEmails: 0,
          },
          {
            id: "mbx-2",
            name: "Archive",
            role: "archive",
            totalEmails: 0,
            unreadEmails: 0,
          },
        ],
        "POST /v1/sources/acct/commands/messages/m1/replace-mailboxes": {
          events: [],
        },
      },
      env: { PH_EVENT_SEQ: "5" },
    });
    const code = await run(
      [
        "move",
        "--account",
        "acct",
        "--message",
        "m1",
        "--to-mailbox",
        "archive",
      ],
      deps,
    );
    expect(code).toBe(ExitCode.Ok);
    expect(cap.calls).toHaveLength(2);
    expect(cap.calls[1]!.body).toEqual({ mailboxIds: ["mbx-2"] });
    expect(cap.calls[1]!.idempotencyKey).toBe("seq:5:move");
  });

  test("a raw mailbox id is accepted directly (no role match required)", async () => {
    const { deps, cap } = harness({
      routes: {
        "GET /v1/sources/acct/mailboxes": [
          {
            id: "mbx-1",
            name: "Inbox",
            role: "inbox",
            totalEmails: 0,
            unreadEmails: 0,
          },
        ],
        "POST /v1/sources/acct/commands/messages/m1/replace-mailboxes": {
          events: [],
        },
      },
    });
    const code = await run(
      ["move", "--account", "acct", "--message", "m1", "--to-mailbox", "mbx-1"],
      deps,
    );
    expect(code).toBe(ExitCode.Ok);
    expect(cap.calls[1]!.body).toEqual({ mailboxIds: ["mbx-1"] });
  });

  test("an unknown role/id is an API-layer usage error, not a crash", async () => {
    const { deps, cap } = harness({
      routes: {
        "GET /v1/sources/acct/mailboxes": [
          {
            id: "mbx-1",
            name: "Inbox",
            role: "inbox",
            totalEmails: 0,
            unreadEmails: 0,
          },
        ],
      },
    });
    const code = await run(
      ["move", "--account", "acct", "--message", "m1", "--to-mailbox", "nope"],
      deps,
    );
    expect(code).toBe(ExitCode.Usage);
    expect(cap.err).toContain("no mailbox with role or id 'nope'");
  });
});

describe("reply — end to end via run()", () => {
  test("fetches reply-context, then sends through it", async () => {
    const { deps, cap } = harness({
      routes: {
        "GET /v1/sources/acct/messages/m1/reply-context": {
          to: [{ email: "sender@example.com", name: "Sender" }],
          cc: [],
          originalTo: [],
          replySubject: "Re: hi",
          forwardSubject: "Fwd: hi",
          quotedBody: null,
          forwardedBody: null,
          inReplyTo: "<abc@example.com>",
          references: "<abc@example.com>",
        },
        "POST /v1/sources/acct/commands/send": { ok: true },
      },
      env: { PH_EVENT_SEQ: "12" },
    });
    const code = await run(
      ["reply", "--account", "acct", "--message", "m1", "--body", "thanks!"],
      deps,
    );
    expect(code).toBe(ExitCode.Ok);
    expect(cap.calls).toHaveLength(2);
    const send = cap.calls[1]!;
    expect(send.body).toMatchObject({
      to: [{ email: "sender@example.com", name: "Sender" }],
      subject: "Re: hi",
      body: "thanks!",
      inReplyTo: "<abc@example.com>",
      references: "<abc@example.com>",
    });
    expect(send.idempotencyKey).toBe("seq:12:reply");
  });

  test("--body '-' reads the reply body from stdin", async () => {
    const { deps, cap } = harness({
      routes: {
        "GET /v1/sources/acct/messages/m1/reply-context": {
          to: [],
          cc: [],
          originalTo: [],
          replySubject: "Re: hi",
          forwardSubject: "Fwd: hi",
          quotedBody: null,
          forwardedBody: null,
          inReplyTo: null,
          references: null,
        },
        "POST /v1/sources/acct/commands/send": { ok: true },
      },
      stdin: "piped body",
    });
    await run(
      ["reply", "--account", "acct", "--message", "m1", "--body", "-"],
      deps,
    );
    expect((cap.calls[1]!.body as { body: string }).body).toBe("piped body");
  });
});

describe("send — end to end via run()", () => {
  test("POSTs a new message with repeatable --to/--cc/--bcc", async () => {
    const { deps, cap } = harness({
      routes: { "POST /v1/sources/acct/commands/send": { ok: true } },
    });
    const code = await run(
      [
        "send",
        "--account",
        "acct",
        "--to",
        "a@x.com",
        "--to",
        "b@x.com",
        "--cc",
        "c@x.com",
        "--subject",
        "Hello",
        "--body",
        "World",
      ],
      deps,
    );
    expect(code).toBe(ExitCode.Ok);
    expect(cap.calls[0]!.body).toMatchObject({
      to: [
        { email: "a@x.com", name: null },
        { email: "b@x.com", name: null },
      ],
      cc: [{ email: "c@x.com", name: null }],
      subject: "Hello",
      body: "World",
    });
  });
});

describe("apply — end to end via run()", () => {
  test("--kind destroy needs no --body", async () => {
    const { deps, cap } = harness({
      routes: {
        "POST /v1/sources/acct/commands/messages/m1/destroy": { events: [] },
      },
    });
    const code = await run(
      ["apply", "--kind", "destroy", "--account", "acct", "--message", "m1"],
      deps,
    );
    expect(code).toBe(ExitCode.Ok);
    expect(cap.calls[0]!.body).toBeUndefined();
  });

  test("--kind set-keywords parses --body as JSON and posts it verbatim", async () => {
    const { deps, cap } = harness({
      routes: {
        "POST /v1/sources/acct/commands/messages/m1/set-keywords": {
          events: [],
        },
      },
    });
    const code = await run(
      [
        "apply",
        "--kind",
        "set-keywords",
        "--account",
        "acct",
        "--message",
        "m1",
        "--body",
        '{"add":["x"],"remove":[]}',
      ],
      deps,
    );
    expect(code).toBe(ExitCode.Ok);
    expect(cap.calls[0]!.body).toEqual({ add: ["x"], remove: [] });
  });

  test("invalid JSON --body is a usage error", async () => {
    const { deps, cap } = harness({ routes: {} });
    const code = await run(
      [
        "apply",
        "--kind",
        "set-keywords",
        "--account",
        "acct",
        "--message",
        "m1",
        "--body",
        "{not json",
      ],
      deps,
    );
    expect(code).toBe(ExitCode.Usage);
    expect(cap.err).toContain("not valid JSON");
  });
});
