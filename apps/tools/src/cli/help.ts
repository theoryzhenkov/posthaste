// Help rendering: the top-level command table and per-command usage, both
// derived from the registry (and its schemas) so help can never drift from
// what the parser accepts.

import type { Operation } from "../operations/index.js";
import { describeFields } from "./schema.js";

const PROG = "posthastectl";

/** Top-level usage: global flags + every command grouped read-then-write. */
export function topLevelHelp(operations: Operation[]): string {
  const lines: string[] = [];
  lines.push(`${PROG} — scriptable CLI for the Posthaste app's local API`);
  lines.push("");
  lines.push(`Usage: ${PROG} [global flags] <command> [args]`);
  lines.push("");
  lines.push("Global flags:");
  lines.push("  --base-url <url>   Backend origin (overrides discovery)");
  lines.push("  -i, --input <src>  JSON args object: inline, '-' (stdin), or '@file'");
  lines.push("  --compact          One-line JSON output (default when piped)");
  lines.push("  --pretty           Indented JSON output (default on a TTY)");
  lines.push("  -h, --help         Show help (top level, or for a command)");
  lines.push("  --version          Print version");
  lines.push("");
  lines.push("Auth: POSTHASTE_TOKEN env or the app's connection-info file — never a flag.");
  lines.push("");

  const extras: Array<[string, string]> = [
    ["events", "Stream the event feed as newline-delimited JSON"],
    ["watch", "Run a command (or emit JSON) per matching event"],
    ["mcp", "Run the stdio MCP server for an agent host"],
  ];
  const width = Math.max(
    ...operations.map((op) => op.cli.path.join(" ").length),
    ...extras.map(([name]) => name.length),
  );

  lines.push("Read commands:");
  for (const op of operations.filter((op) => !op.mutates)) {
    lines.push(`  ${op.cli.path.join(" ").padEnd(width)}  ${firstSentence(op.description)}`);
  }
  lines.push("");
  lines.push("Write commands (mutate mail state):");
  for (const op of operations.filter((op) => op.mutates)) {
    lines.push(`  ${op.cli.path.join(" ").padEnd(width)}  ${firstSentence(op.description)}`);
  }
  lines.push("");
  lines.push("Streaming & agents:");
  for (const [name, blurb] of extras) {
    lines.push(`  ${name.padEnd(width)}  ${blurb}`);
  }
  lines.push("");
  lines.push(`Run '${PROG} <command> --help' for command details.`);
  return lines.join("\n");
}

/** Per-command usage: signature, description, and flag list from the schema. */
export function commandHelp(op: Operation): string {
  const path = op.cli.path.join(" ");
  const fields = describeFields(op.argSchema);
  const lines: string[] = [];

  const sig = fields
    .map((f) => {
      const token = op.cli.primary === f.name ? `<${f.name}>` : `--${f.flag}`;
      return f.required ? token : `[${token}]`;
    })
    .join(" ");
  lines.push(`Usage: ${PROG} ${path}${sig ? ` ${sig}` : ""}`);
  lines.push("");
  lines.push(op.description);
  if (op.mutates) {
    lines.push("");
    lines.push("(mutating — changes mail state; retries are safe via --id)");
  }
  if (fields.length > 0) {
    lines.push("");
    lines.push("Arguments:");
    const width = Math.max(...fields.map((f) => f.flag.length));
    for (const f of fields) {
      const flag = `--${f.flag}`.padEnd(width + 2);
      const meta = `${f.kind}${f.required ? ", required" : ""}`;
      const primary = op.cli.primary === f.name ? " (positional)" : "";
      const blurb = f.description ? ` — ${f.description}` : "";
      lines.push(`  ${flag}  ${meta}${primary}${blurb}`);
    }
    lines.push("");
    lines.push(
      "Complex (array/object) args are JSON: pass via the flag value or -i/--input.",
    );
  }
  return lines.join("\n");
}

function firstSentence(text: string): string {
  const stripped = text.replace(/^WRITE: /, "");
  const dot = stripped.indexOf(". ");
  return dot >= 0 ? stripped.slice(0, dot + 1) : stripped;
}
