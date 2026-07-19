import { describe, expect, test } from "bun:test";

import { operations } from "../src/operations/index.js";
import { parseRecipient, reply, setKeywords } from "../src/operations/commands.js";
import { searchMessages, listMessages } from "../src/operations/read.js";
import { fakeConnection, fakeFetch } from "./helpers.js";

describe("registry", () => {
  test("tool names are unique snake_case; CLI paths are unique", () => {
    const names = operations.map((op) => op.mcpName);
    expect(new Set(names).size).toBe(names.length);
    for (const name of names) expect(name).toMatch(/^[a-z][a-z0-9_]*$/);
    const paths = operations.map((op) => op.cli.path.join(" "));
    expect(new Set(paths).size).toBe(paths.length);
  });

  test("write operations are described as WRITE and carry the retry id field", () => {
    for (const op of operations.filter((op) => op.mutates)) {
      expect(op.description.startsWith("WRITE:")).toBe(true);
      expect(Object.keys(op.argSchema)).toContain("id");
    }
    // The ported tool set survives, including the new pending-operations read.
    const names = operations.map((op) => op.mcpName);
    for (const expected of [
      "list_accounts",
      "search_messages",
      "get_message",
      "get_thread",
      "list_pending_operations",
      "create_mailbox",
      "delete_mailbox",
      "set_keywords",
      "move_to_mailbox",
      "send_message",
      "reply",
      "trigger_sync",
    ]) {
      expect(names).toContain(expected);
    }
  });
});

describe("read handlers", () => {
  test("search renders as a windowed mailList query with freeText", async () => {
    const { fetch, requests } = fakeFetch([
      { status: 200, json: { generation: 1, data: { rows: [], nextCursor: null } } },
    ]);
    await searchMessages.handler(fakeConnection(fetch), {
      query: "from:alice is:unread",
      limit: 25,
      cursor: "abc",
    });
    expect(requests[0]?.body).toEqual({
      mailList: {
        accountId: null,
        mailboxId: null,
        freeText: "from:alice is:unread",
        isRead: null,
        isFlagged: null,
        hasAttachment: null,
        limit: 25,
        cursor: "abc",
      },
    });
  });

  test("list passes windowing and filters through", async () => {
    const { fetch, requests } = fakeFetch([
      { status: 200, json: { generation: 1, data: { rows: [], nextCursor: "next" } } },
    ]);
    const result = (await listMessages.handler(fakeConnection(fetch), {
      accountId: "a1",
      mailboxId: "mb1",
      isRead: false,
      limit: 10,
    })) as { data: { nextCursor: string | null } };
    const body = requests[0]?.body as { mailList: Record<string, unknown> };
    expect(body.mailList.accountId).toBe("a1");
    expect(body.mailList.isRead).toBe(false);
    expect(body.mailList.limit).toBe(10);
    expect(result.data.nextCursor).toBe("next");
  });
});

describe("write handlers", () => {
  test("set_keywords composes the typed intent and returns {id, generation}", async () => {
    const { fetch, requests } = fakeFetch([{ status: 200, json: { generation: 42 } }]);
    const result = (await setKeywords.handler(fakeConnection(fetch), {
      accountId: "a1",
      messageId: "m1",
      add: ["$flagged"],
    })) as { id: string; generation: number };
    expect(result.generation).toBe(42);
    expect(result.id).toMatch(/^[0-9A-HJKMNP-TV-Z]{26}$/);
    const body = requests[0]?.body as { id: string; command: unknown };
    expect(requests[0]?.url).toBe("http://127.0.0.1:9/command");
    expect(body.command).toEqual({
      setKeywords: {
        accountId: "a1",
        messageId: "m1",
        change: { add: ["$flagged"], remove: [] },
      },
    });
  });

  test("set_keywords with nothing to change is rejected before the wire", async () => {
    const { fetch, requests } = fakeFetch([]);
    await expect(
      Promise.resolve(
        setKeywords.handler(fakeConnection(fetch), { accountId: "a1", messageId: "m1" }),
      ),
    ).rejects.toThrow("at least one keyword");
    expect(requests).toHaveLength(0);
  });

  test("reply reads the original, then sends with threading headers", async () => {
    const { fetch, requests } = fakeFetch([
      {
        status: 200,
        json: {
          generation: 5,
          data: {
            summary: {
              subject: "Plan",
              fromName: "Alice",
              fromEmail: "alice@example.com",
              rfcMessageId: "<orig@x>",
              inReplyTo: "<earlier@x>",
            },
          },
        },
      },
      { status: 200, json: { generation: 6 } },
    ]);
    const result = (await reply.handler(fakeConnection(fetch), {
      accountId: "a1",
      messageId: "m1",
      body: "sounds good",
    })) as { id: string; generation: number };
    expect(result.generation).toBe(6);

    expect(requests[0]?.url).toContain("/query");
    expect(requests[0]?.body).toEqual({
      messageDetail: { accountId: "a1", messageId: "m1" },
    });

    const command = (requests[1]?.body as { command: { send: { request: Record<string, unknown> } } })
      .command;
    const request = command.send.request;
    expect(request.to).toEqual([{ name: "Alice", email: "alice@example.com" }]);
    expect(request.subject).toBe("Re: Plan");
    expect(request.inReplyTo).toBe("<orig@x>");
    expect(request.references).toBe("<earlier@x> <orig@x>");
    expect(request.body).toBe("sounds good");
  });

  test("parseRecipient handles named and bare addresses", () => {
    expect(parseRecipient("Alice <alice@example.com>")).toEqual({
      name: "Alice",
      email: "alice@example.com",
    });
    expect(parseRecipient("bob@example.com")).toEqual({
      name: null,
      email: "bob@example.com",
    });
  });
});
