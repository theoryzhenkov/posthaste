import { describe, expect, test } from "bun:test";

import type { Connection } from "../src/client.js";
import { operations } from "../src/operations/index.js";

interface Call {
  url: string;
  method: string;
  body: unknown;
  idempotencyKey?: string;
}

/**
 * A mock daemon: `GET …/reply-context` returns a fixed reply context; `POST
 * …/commands/send` records the composed request and acks.
 */
function harness(): { conn: Connection; calls: Call[] } {
  const calls: Call[] = [];
  const fetch: typeof globalThis.fetch = async (input, init) => {
    const url = String(input);
    const method = init?.method ?? "GET";
    const headers = new Headers(init?.headers);
    calls.push({
      url,
      method,
      body: init?.body ? JSON.parse(String(init.body)) : undefined,
      idempotencyKey: headers.get("idempotency-key") ?? undefined,
    });
    if (url.endsWith("/reply-context")) {
      return Response.json({
        to: [{ email: "alice@example.com", name: "Alice" }],
        cc: [],
        originalTo: [],
        replySubject: "Re: Hello",
        forwardSubject: "Fwd: Hello",
        inReplyTo: "<orig@example.com>",
        references: "<thread@example.com>",
      });
    }
    return Response.json({ ok: true });
  };
  const conn: Connection = {
    baseUrl: "http://daemon/v1",
    token: "scoped",
    source: "daemon.json",
    fetch,
  };
  return { conn, calls };
}

const reply = operations.find((op) => op.mcpName === "reply")!;

describe("reply tool", () => {
  test("is registered as a mutating operation", () => {
    expect(reply).toBeDefined();
    expect(reply.mutates).toBe(true);
    expect(reply.cli.path).toEqual(["messages", "reply"]);
  });

  test("looks up reply-context, then sends the composed reply", async () => {
    const { conn, calls } = harness();
    const result = await reply.handler(conn, {
      sourceId: "acct-a",
      messageId: "msg-1",
      body: "Got it, thanks!",
    });
    expect(result).toEqual({ ok: true });

    expect(calls).toHaveLength(2);
    expect(calls[0]!.method).toBe("GET");
    expect(calls[0]!.url).toContain(
      "/sources/acct-a/messages/msg-1/reply-context",
    );

    const send = calls[1]!;
    expect(send.method).toBe("POST");
    expect(send.url).toContain("/sources/acct-a/commands/send");
    const body = send.body as {
      to: { email: string }[];
      subject: string;
      body: string;
      inReplyTo?: string;
      references?: string;
    };
    expect(body.to).toEqual([{ email: "alice@example.com", name: "Alice" }]);
    expect(body.subject).toBe("Re: Hello");
    expect(body.body).toBe("Got it, thanks!");
    expect(body.inReplyTo).toBe("<orig@example.com>");
    expect(body.references).toBe("<thread@example.com>");
  });

  test("forwards an explicit idempotency key on the send", async () => {
    const { conn, calls } = harness();
    await reply.handler(conn, {
      sourceId: "acct-a",
      messageId: "msg-1",
      body: "hi",
      idempotencyKey: "rule:r1:seq:5:reply",
    });
    expect(calls[1]!.idempotencyKey).toBe("rule:r1:seq:5:reply");
  });
});
