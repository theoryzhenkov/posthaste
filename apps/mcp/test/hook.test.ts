import { afterEach, describe, expect, test } from "bun:test";

import {
  deriveHookEnv,
  startHookServer,
  type HookServerHandle,
} from "../src/cli/hook.js";

/**
 * A representative Level-1 hook delivery body (mirrors `run_hook`'s payload in
 * `crates/posthaste-authority-server/src/rules/actions.rs`).
 */
function payload(overrides: Record<string, unknown> = {}): string {
  return JSON.stringify({
    ruleId: "instruct-agent",
    idempotencyKey: "rule:instruct-agent:91",
    event: {
      seq: 91,
      topic: "message.updated",
      accountId: "acct-1",
      mailboxId: null,
      messageId: "msg-1",
    },
    message: {
      id: "msg-1",
      sourceId: "acct-1",
      subject: "Re: invoice",
      fromName: "Ada Lovelace",
      fromEmail: "ada@example.com",
      keywords: ["instruct", "urgent"],
    },
    token: "attenuated-macaroon",
    ...overrides,
  });
}

describe("deriveHookEnv — mirrors the exec action's PH_* set + POSTHASTE_*", () => {
  test("derives the full PH_* set from a well-formed delivery", () => {
    const env = deriveHookEnv(payload(), "http://127.0.0.1:3001/v1");
    expect(env.PH_RULE).toBe("instruct-agent");
    expect(env.PH_IDEMPOTENCY_KEY).toBe("rule:instruct-agent:91");
    expect(env.PH_ACCOUNT).toBe("acct-1");
    expect(env.PH_MESSAGE_ID).toBe("msg-1");
    expect(env.PH_FROM).toBe("ada@example.com");
    expect(env.PH_SUBJECT).toBe("Re: invoice");
    expect(env.PH_KEYWORDS).toBe("instruct,urgent");
    expect(env.PH_EVENT_SEQ).toBe("91");
    expect(env.PH_TOPIC).toBe("message.updated");
    expect(env.POSTHASTE_TOKEN).toBe("attenuated-macaroon");
    expect(env.POSTHASTE_API_URL).toBe("http://127.0.0.1:3001/v1");
  });

  test("PH_FROM falls back to the display name, then empties", () => {
    const noEmail = deriveHookEnv(
      payload({ message: { fromName: "Ada", keywords: [] } }),
      undefined,
    );
    expect(noEmail.PH_FROM).toBe("Ada");

    const neither = deriveHookEnv(
      payload({ message: { keywords: [] } }),
      undefined,
    );
    expect(neither.PH_FROM).toBe("");
  });

  test("no apiUrl discovered → POSTHASTE_API_URL is omitted", () => {
    const env = deriveHookEnv(payload(), undefined);
    expect(env.POSTHASTE_API_URL).toBeUndefined();
  });

  test("no token in the payload → POSTHASTE_TOKEN is omitted", () => {
    const env = deriveHookEnv(payload({ token: undefined }), undefined);
    expect(env.POSTHASTE_TOKEN).toBeUndefined();
  });

  test("malformed JSON never throws — every PH_* is empty", () => {
    const env = deriveHookEnv("not json { at all", undefined);
    expect(env).toEqual({
      PH_RULE: "",
      PH_IDEMPOTENCY_KEY: "",
      PH_ACCOUNT: "",
      PH_MESSAGE_ID: "",
      PH_FROM: "",
      PH_SUBJECT: "",
      PH_KEYWORDS: "",
      PH_EVENT_SEQ: "",
      PH_TOPIC: "",
    });
  });

  test("a non-object JSON body (e.g. a bare array) never throws", () => {
    const env = deriveHookEnv("[1,2,3]", undefined);
    expect(env.PH_RULE).toBe("");
  });
});

describe("hook serve — live server", () => {
  let handle: HookServerHandle | undefined;

  afterEach(() => {
    handle?.stop();
    handle = undefined;
  });

  interface Delivery {
    command: string;
    input: string;
    env: Record<string, string>;
  }

  function harness(opts: { token?: string; exitCode?: number } = {}): {
    deliveries: Delivery[];
    logs: string[];
    start: (extra?: { path?: string }) => HookServerHandle;
  } {
    const deliveries: Delivery[] = [];
    const logs: string[] = [];
    const start = (extra: { path?: string } = {}): HookServerHandle => {
      handle = startHookServer(
        {
          exec: "./handler.sh",
          port: 0, // OS-assigned, so parallel tests never collide
          path: extra.path,
          token: opts.token,
        },
        {
          runCommand: async (command, input, env) => {
            deliveries.push({ command, input, env });
            return opts.exitCode ?? 0;
          },
          log: (line) => logs.push(line),
          apiUrl: "http://127.0.0.1:3001/v1",
        },
      );
      return handle;
    };
    return { deliveries, logs, start };
  }

  test("binds 127.0.0.1 and answers POST /hook by default", async () => {
    const h = harness();
    const server = h.start();
    expect(server.url).toBe(`http://127.0.0.1:${server.port}/hook`);

    const res = await fetch(server.url, {
      method: "POST",
      body: payload(),
    });
    expect(res.status).toBe(200);
    expect(h.deliveries).toHaveLength(1);
  });

  test("--path overrides the delivery path; other paths 404", async () => {
    const h = harness();
    const server = h.start({ path: "/webhooks/instruct" });
    expect(server.url).toEndWith("/webhooks/instruct");

    const wrongPath = await fetch(`http://127.0.0.1:${server.port}/hook`, {
      method: "POST",
      body: payload(),
    });
    expect(wrongPath.status).toBe(404);
    expect(h.deliveries).toHaveLength(0);

    const rightPath = await fetch(server.url, {
      method: "POST",
      body: payload(),
    });
    expect(rightPath.status).toBe(200);
    expect(h.deliveries).toHaveLength(1);
  });

  test("delivers the body on stdin and derives PH_* env, not argv", async () => {
    const h = harness();
    const server = h.start();
    const body = payload();
    await fetch(server.url, { method: "POST", body });

    expect(h.deliveries).toHaveLength(1);
    const delivery = h.deliveries[0]!;
    // The command string itself is untouched — no interpolation of any field.
    expect(delivery.command).toBe("./handler.sh");
    expect(delivery.input).toBe(body);
    expect(JSON.parse(delivery.input)).toEqual(JSON.parse(body));
    expect(delivery.env.PH_RULE).toBe("instruct-agent");
    expect(delivery.env.PH_MESSAGE_ID).toBe("msg-1");
    expect(delivery.env.POSTHASTE_TOKEN).toBe("attenuated-macaroon");
    expect(delivery.env.POSTHASTE_API_URL).toBe("http://127.0.0.1:3001/v1");
  });

  /**
   * Payload-is-data (ruling 20a): a body containing shell metacharacters must
   * reach the script byte-for-byte on stdin — never interpreted, never
   * substituted into the command string or split onto argv.
   */
  test("a body with shell metacharacters reaches the script verbatim on stdin, never shell-interpolated", async () => {
    const h = harness();
    const server = h.start();
    const hostile = payload({
      ruleId: "$(rm -rf /); `touch pwned`; ${IFS}&& echo hi | sh",
    });
    const res = await fetch(server.url, { method: "POST", body: hostile });
    expect(res.status).toBe(200);

    const delivery = h.deliveries[0]!;
    // The command run is always the fixed --exec script — never rewritten
    // with any part of the payload.
    expect(delivery.command).toBe("./handler.sh");
    // stdin carries the hostile bytes completely unchanged.
    expect(delivery.input).toBe(hostile);
    // The metacharacters also survive into the derived env var untouched —
    // proof they were never passed through a shell.
    expect(delivery.env.PH_RULE).toBe(
      "$(rm -rf /); `touch pwned`; ${IFS}&& echo hi | sh",
    );
  });

  test("no --token set → any request is accepted", async () => {
    const h = harness();
    const server = h.start();
    const res = await fetch(server.url, { method: "POST", body: payload() });
    expect(res.status).toBe(200);
  });

  test("--token set: a matching bearer is accepted", async () => {
    const h = harness({ token: "s3cret" });
    const server = h.start();
    const res = await fetch(server.url, {
      method: "POST",
      body: payload(),
      headers: { authorization: "Bearer s3cret" },
    });
    expect(res.status).toBe(200);
    expect(h.deliveries).toHaveLength(1);
  });

  test("--token set: an absent bearer is rejected 401, handler never runs", async () => {
    const h = harness({ token: "s3cret" });
    const server = h.start();
    const res = await fetch(server.url, { method: "POST", body: payload() });
    expect(res.status).toBe(401);
    expect(h.deliveries).toHaveLength(0);
    expect(h.logs.join("\n")).toContain("missing or wrong bearer token");
  });

  test("--token set: a wrong bearer is rejected 401, handler never runs", async () => {
    const h = harness({ token: "s3cret" });
    const server = h.start();
    const res = await fetch(server.url, {
      method: "POST",
      body: payload(),
      headers: { authorization: "Bearer wrong-token" },
    });
    expect(res.status).toBe(401);
    expect(h.deliveries).toHaveLength(0);
  });

  test("a non-zero handler exit is logged and reported 500", async () => {
    const h = harness({ exitCode: 3 });
    const server = h.start();
    const res = await fetch(server.url, { method: "POST", body: payload() });
    expect(res.status).toBe(500);
    expect(h.logs.join("\n")).toContain("exited 3");
  });

  test("GET (or any non-POST) to the hook path is 404, handler never runs", async () => {
    const h = harness();
    const server = h.start();
    const res = await fetch(server.url, { method: "GET" });
    expect(res.status).toBe(404);
    expect(h.deliveries).toHaveLength(0);
  });
});
