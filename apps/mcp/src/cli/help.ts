import type { Operation } from "../operations/index.js";
import { describeFields } from "./schema.js";

const PROG = "posthastectl";

/** Top-level usage: global flags + every command grouped by its first segment. */
export function topLevelHelp(operations: Operation[]): string {
  const lines: string[] = [];
  lines.push(`${PROG} — scriptable CLI for the Posthaste daemon (/v1 API)`);
  lines.push("");
  lines.push(`Usage: ${PROG} [global flags] <command> [args]`);
  lines.push("");
  lines.push("Global flags:");
  lines.push(
    "  --base-url <url>   Daemon base URL incl. /v1 (overrides discovery)",
  );
  lines.push("  --token <token>    Bearer token (overrides discovery)");
  lines.push(
    "  -i, --input <src>  JSON args object: inline, '-' (stdin), or '@file'",
  );
  lines.push("  --compact          One-line JSON output (default when piped)");
  lines.push("  --pretty           Indented JSON output (default on a TTY)");
  lines.push("  -h, --help         Show help (top level, or for a command)");
  lines.push("  --version          Print version");
  lines.push("");
  lines.push("Commands:");

  const width = Math.max(
    ...operations.map((op) => op.cli.path.join(" ").length),
    "events".length,
  );
  for (const op of operations) {
    const path = op.cli.path.join(" ").padEnd(width);
    lines.push(`  ${path}  ${firstSentence(op.description)}`);
  }
  lines.push(
    `  ${"events".padEnd(width)}  Stream domain events as newline-delimited JSON`,
  );
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
    lines.push("(mutating — changes server state)");
  }
  if (fields.length > 0) {
    lines.push("");
    lines.push("Arguments:");
    const width = Math.max(...fields.map((f) => f.flag.length));
    for (const f of fields) {
      const flag = `--${f.flag}`.padEnd(width + 2);
      const meta = `${f.kind}${f.required ? ", required" : ""}`;
      const primary = op.cli.primary === f.name ? " (positional)" : "";
      lines.push(`  ${flag}  ${meta}${primary}`);
    }
    lines.push("");
    lines.push(
      "Complex (array/object) args are JSON: pass via the flag value or -i/--input.",
    );
  }
  return lines.join("\n");
}

function firstSentence(text: string): string {
  const dot = text.indexOf(". ");
  return dot >= 0 ? text.slice(0, dot + 1) : text;
}
