// The zod-introspecting argument framework: an operation's leftover tokens
// (flags + one optional positional) are parsed, coerced by field kind, seeded
// from `--input`, and validated by the operation's own schema.

import { z } from "zod";

import type { Operation } from "../operations/index.js";
import { describeFields, kebabToCamel, type FieldInfo } from "./schema.js";

/** A usage error: the message is printed to stderr and the CLI exits with 2. */
export class UsageError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "UsageError";
  }
}

/** Coerce a raw string flag value to the JS type its field expects. */
function coerceScalar(value: string, kind: FieldInfo["kind"]): unknown {
  if (kind === "number") {
    const n = Number(value);
    return Number.isNaN(n) ? value : n;
  }
  if (kind === "boolean") {
    if (value === "true") return true;
    if (value === "false") return false;
    return value;
  }
  if (kind === "array" || kind === "object") {
    try {
      return JSON.parse(value);
    } catch {
      return value;
    }
  }
  return value;
}

/** Coerce the collected raw values for one flag into the field's value. */
function coerceField(values: string[], field: FieldInfo): unknown {
  if (field.kind === "array") {
    // A single `[`/`{`-prefixed value is JSON; otherwise repeated `--flag a
    // --flag b` collects into an array (with scalar element coercion).
    if (values.length === 1 && /^\s*[[{]/.test(values[0] ?? "")) {
      return coerceScalar(values[0] ?? "", "array");
    }
    return values.map((v) => coerceScalar(v, field.elementKind ?? "string"));
  }
  // Scalars: the last occurrence wins.
  return coerceScalar(values[values.length - 1] ?? "", field.kind);
}

/** Format a zod validation error into a single human-readable line. */
function formatZodError(error: z.ZodError): string {
  return error.issues
    .map((issue) => {
      const path = issue.path.join(".");
      return path ? `${path}: ${issue.message}` : issue.message;
    })
    .join("; ");
}

/**
 * Parse an operation's leftover CLI tokens into a validated args object.
 *
 * Tokens are a mix of `--flag[=value]` and positionals. Flag values are taken
 * from `=value`, the next token, or (for booleans) implied `true`. An
 * optional JSON `inputObject` (from `--input`) seeds the args; explicit flags
 * override it; a single bare positional fills the operation's `primary` field.
 */
export function parseOperationArgs(
  op: Operation,
  tokens: string[],
  inputObject: Record<string, unknown> | undefined,
): Record<string, unknown> {
  const fields = describeFields(op.argSchema);
  const byFlag = new Map(fields.map((f) => [f.flag, f]));

  const collected = new Map<string, string[]>();
  const positionals: string[] = [];

  for (let i = 0; i < tokens.length; i++) {
    const token = tokens[i] ?? "";
    if (!token.startsWith("--")) {
      positionals.push(token);
      continue;
    }
    const body = token.slice(2);
    const eq = body.indexOf("=");
    const flag = eq >= 0 ? body.slice(0, eq) : body;
    const field = byFlag.get(flag);
    if (!field) {
      throw new UsageError(
        `unknown flag --${flag} for '${op.cli.path.join(" ")}'`,
      );
    }
    let value: string;
    if (eq >= 0) {
      value = body.slice(eq + 1);
    } else if (field.kind === "boolean") {
      value = "true";
    } else {
      const next = tokens[i + 1];
      if (next === undefined || next.startsWith("--")) {
        throw new UsageError(`flag --${flag} requires a value`);
      }
      value = next;
      i++;
    }
    const existing = collected.get(flag);
    if (existing) existing.push(value);
    else collected.set(flag, [value]);
  }

  // Seed from --input, then apply flags (flags win).
  const raw: Record<string, unknown> = { ...(inputObject ?? {}) };
  for (const [flag, values] of collected) {
    const field = byFlag.get(flag);
    if (field) raw[field.name] = coerceField(values, field);
  }

  // A single bare positional fills the `primary` field (if not already set).
  if (positionals.length > 0) {
    const primary = op.cli.primary;
    if (!primary) {
      throw new UsageError(
        `'${op.cli.path.join(" ")}' takes no positional arguments ` +
          `(got ${positionals.map((p) => JSON.stringify(p)).join(", ")})`,
      );
    }
    if (positionals.length > 1) {
      throw new UsageError(
        `'${op.cli.path.join(" ")}' takes a single positional ${primary}`,
      );
    }
    if (raw[primary] === undefined) {
      const field = fields.find((f) => f.name === primary);
      raw[primary] = field
        ? coerceField([positionals[0] ?? ""], field)
        : positionals[0];
    }
  }

  const parsed = z.object(op.argSchema).safeParse(raw);
  if (!parsed.success) {
    throw new UsageError(formatZodError(parsed.error));
  }
  return parsed.data as Record<string, unknown>;
}

export { kebabToCamel };
