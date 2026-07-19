// Guide smoke test: every `posthastectl ...` example in the site guides must
// parse against the real registry — command path, flags, and positional
// arity — so the docs cannot drift from the CLI again. It also pins a few
// facts the guides state about the environment contract.

import { describe, expect, test } from "bun:test";
import { readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";

import { operations } from "../src/operations/index.js";
import { describeFields } from "../src/cli/schema.js";

const GUIDE_DIR = join(import.meta.dir, "..", "..", "site", "src", "content", "guide");

/** Global flags handled by the dispatcher before operation parsing. */
const GLOBAL_VALUED = new Set(["--base-url", "--input", "-i"]);
const GLOBAL_BARE = new Set([
  "--compact",
  "--json",
  "--pretty",
  "--help",
  "-h",
  "--version",
  "-V",
]);

/** The streaming commands' flags (mirrors cli/run.ts parseEvents/WatchOptions). */
const STREAMING: Record<string, { valued: Set<string>; bare: Set<string> }> = {
  events: {
    valued: new Set(["--kind", "--account", "--mailbox"]),
    bare: new Set(["--generation-only"]),
  },
  watch: {
    valued: new Set(["--account", "--kind", "--mailbox", "--keyword", "--exec"]),
    bare: new Set(["--all-updates", "--reconnect"]),
  },
};

/** Extract the bodies of ```sh fenced blocks. */
function shBlocks(markdown: string): string[] {
  const blocks: string[] = [];
  const re = /```sh\n([\s\S]*?)```/g;
  for (let m = re.exec(markdown); m; m = re.exec(markdown)) {
    blocks.push(m[1] ?? "");
  }
  return blocks;
}

/** Join `\`-continued lines and return the posthastectl invocations. */
function invocations(block: string): string[] {
  const logical: string[] = [];
  let pending = "";
  for (const raw of block.split("\n")) {
    const joined = pending + raw;
    if (joined.endsWith("\\")) {
      pending = `${joined.slice(0, -1)} `;
      continue;
    }
    pending = "";
    logical.push(joined);
  }
  return logical
    .map((line) => line.trim())
    .filter((line) => line.startsWith("posthastectl"));
}

/** Quote-aware tokenizer; stops at an unquoted comment or pipe. */
function tokenize(line: string): string[] {
  const tokens: string[] = [];
  let current = "";
  let quote: "'" | '"' | undefined;
  const push = () => {
    if (current.length > 0) tokens.push(current);
    current = "";
  };
  for (let i = 0; i < line.length; i++) {
    const ch = line[i] as string;
    if (quote) {
      current += ch;
      if (ch === quote) quote = undefined;
      continue;
    }
    if (ch === "'" || ch === '"') {
      quote = ch;
      current += ch;
      continue;
    }
    if (/\s/.test(ch)) {
      push();
      continue;
    }
    if ((ch === "#" || ch === "|") && current.length === 0) {
      break; // comment / pipeline tail — the invocation is complete
    }
    current += ch;
  }
  push();
  return tokens;
}

/** Validate one tokenized invocation against the registry; returns errors. */
function validate(tokens: string[]): string[] {
  const errors: string[] = [];
  const rest: string[] = [];
  // Peel globals, as the dispatcher does.
  for (let i = 1; i < tokens.length; i++) {
    const token = tokens[i] as string;
    if (GLOBAL_VALUED.has(token)) {
      i++;
      continue;
    }
    if (GLOBAL_BARE.has(token)) continue;
    rest.push(token);
  }

  const head = rest[0];
  if (head === "mcp") return errors;

  const streaming = head ? STREAMING[head] : undefined;
  if (streaming) {
    for (let i = 1; i < rest.length; i++) {
      const token = rest[i] as string;
      if (streaming.valued.has(token)) {
        if (rest[i + 1] === undefined) errors.push(`${token} requires a value`);
        i++;
      } else if (!streaming.bare.has(token)) {
        errors.push(`unknown ${head} token ${token}`);
      }
    }
    return errors;
  }

  // Longest-path operation match, as cli/run.ts does.
  const leading: string[] = [];
  for (const token of rest) {
    if (token.startsWith("-")) break;
    leading.push(token);
  }
  const op = operations
    .filter(
      (o) =>
        o.cli.path.length <= leading.length &&
        o.cli.path.every((seg, i) => seg === leading[i]),
    )
    .sort((a, b) => b.cli.path.length - a.cli.path.length)[0];
  if (!op) {
    errors.push(`no operation matches '${rest.join(" ")}'`);
    return errors;
  }

  const fields = new Map(describeFields(op.argSchema).map((f) => [`--${f.flag}`, f]));
  let positionals = 0;
  for (let i = op.cli.path.length; i < rest.length; i++) {
    const token = rest[i] as string;
    if (!token.startsWith("--")) {
      positionals++;
      continue;
    }
    const field = fields.get(token);
    if (!field) {
      errors.push(`unknown flag ${token} for '${op.cli.path.join(" ")}'`);
      continue;
    }
    if (field.kind !== "boolean") {
      if (rest[i + 1] === undefined) errors.push(`${token} requires a value`);
      i++;
    }
  }
  if (positionals > 0 && !op.cli.primary) {
    errors.push(`'${op.cli.path.join(" ")}' takes no positional arguments`);
  }
  if (positionals > 1) {
    errors.push(`'${op.cli.path.join(" ")}' takes a single positional`);
  }
  return errors;
}

const guides = readdirSync(GUIDE_DIR).filter((name) => name.endsWith(".md"));

describe("guide examples match the CLI surface", () => {
  test("guides exist", () => {
    expect(guides.length).toBeGreaterThan(0);
  });

  for (const guide of guides) {
    test(guide, () => {
      const markdown = readFileSync(join(GUIDE_DIR, guide), "utf8");
      const failures: string[] = [];
      for (const block of shBlocks(markdown)) {
        for (const line of invocations(block)) {
          const errors = validate(tokenize(line));
          for (const error of errors) failures.push(`${line}\n    ${error}`);
        }
      }
      expect(failures).toEqual([]);
    });
  }

  test("guides state the real environment contract", () => {
    for (const guide of guides) {
      const markdown = readFileSync(join(GUIDE_DIR, guide), "utf8");
      // The watcher exports PH_KIND (not the legacy PH_TOPIC), and discovery
      // overrides are POSTHASTE_API_URL (there is no port variable).
      expect(markdown).not.toContain("PH_TOPIC");
      expect(markdown).not.toContain("POSTHASTE_PORT");
    }
  });
});
