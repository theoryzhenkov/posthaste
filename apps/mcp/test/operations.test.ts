import { describe, expect, test } from "bun:test";

import { operations } from "../src/operations/index.js";
import { describeFields } from "../src/cli/schema.js";

describe("operation registry", () => {
  test("mcp tool names are unique and non-empty", () => {
    const names = operations.map((op) => op.mcpName);
    expect(new Set(names).size).toBe(names.length);
    for (const name of names) expect(name.length).toBeGreaterThan(0);
  });

  test("the original MCP tool names are preserved (a documented contract)", () => {
    const names = new Set(operations.map((op) => op.mcpName));
    for (const original of [
      "list_accounts",
      "read_mail_navigation",
      "list_conversations",
      "get_conversation",
      "search_messages",
      "get_message",
      "set_keywords",
      "move_to_mailbox",
      "send_message",
    ]) {
      expect(names.has(original)).toBe(true);
    }
  });

  test("cli command paths are unique and well-formed", () => {
    const joined = operations.map((op) => op.cli.path.join(" "));
    expect(new Set(joined).size).toBe(joined.length);
    for (const op of operations) {
      expect(op.cli.path.length).toBeGreaterThan(0);
      for (const seg of op.cli.path) expect(seg).toMatch(/^[a-z][a-z-]*$/);
    }
  });

  test("'events' is not shadowed by an operation path", () => {
    for (const op of operations) expect(op.cli.path[0]).not.toBe("events");
  });

  test("every operation has a title, description, and zod arg shape", () => {
    for (const op of operations) {
      expect(op.title.length).toBeGreaterThan(0);
      expect(op.description.length).toBeGreaterThan(0);
      for (const schema of Object.values(op.argSchema)) {
        expect(typeof (schema as { parse?: unknown }).parse).toBe("function");
      }
    }
  });

  test("primary positional, when set, names a real field", () => {
    for (const op of operations) {
      if (!op.cli.primary) continue;
      const fields = describeFields(op.argSchema).map((f) => f.name);
      expect(fields).toContain(op.cli.primary);
    }
  });
});
