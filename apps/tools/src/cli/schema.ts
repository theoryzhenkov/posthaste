// Zod-shape introspection: flatten an operation's arg schema into flag
// descriptions, so CLI flags are derived from the same schema the MCP server
// registers — one schema, both front-ends.

import { z } from "zod";

import type { ArgShape } from "../operations/index.js";

/** The CLI-level kind of an argument field, after unwrapping zod wrappers. */
export type FieldKind = "string" | "number" | "boolean" | "array" | "object";

/** A flattened description of one argument field, for flag rendering + coercion. */
export interface FieldInfo {
  /** camelCase field name (as the schema + API use it). */
  name: string;
  /** Kebab-case flag name (`accountId` → `account-id`). */
  flag: string;
  kind: FieldKind;
  /** True when the schema rejects `undefined` (i.e. a required flag). */
  required: boolean;
  /** For array fields, the element kind (drives repeated-flag collection). */
  elementKind?: FieldKind;
  /** The schema's `.describe(...)` text, for help output. */
  description?: string;
}

/** `accountId` → `account-id`. */
export function camelToKebab(name: string): string {
  return name.replace(/[A-Z]/g, (c) => `-${c.toLowerCase()}`);
}

/** `account-id` → `accountId`. */
export function kebabToCamel(flag: string): string {
  return flag.replace(/-([a-z0-9])/g, (_, c: string) => c.toUpperCase());
}

/** Peel `optional`/`nullable`/`default` wrappers off a zod schema. */
function unwrap(schema: z.ZodTypeAny): z.ZodTypeAny {
  let cur: z.ZodTypeAny = schema;
  for (let i = 0; i < 10; i++) {
    if (cur instanceof z.ZodOptional || cur instanceof z.ZodNullable) {
      cur = cur.unwrap() as z.ZodTypeAny;
      continue;
    }
    if (cur instanceof z.ZodDefault) {
      const inner = (cur as unknown as { def?: { innerType?: z.ZodTypeAny } })
        .def?.innerType;
      if (inner) {
        cur = inner;
        continue;
      }
    }
    break;
  }
  return cur;
}

/** Classify a (possibly wrapped) zod schema into a CLI field kind. */
function kindOf(schema: z.ZodTypeAny): FieldKind {
  const core = unwrap(schema);
  if (core instanceof z.ZodNumber) return "number";
  if (core instanceof z.ZodBoolean) return "boolean";
  if (core instanceof z.ZodArray) return "array";
  if (core instanceof z.ZodObject) return "object";
  // ZodString, ZodEnum, ZodLiteral, and anything else are scalar strings at
  // the CLI boundary (zod still validates the concrete constraint).
  return "string";
}

/** The element kind of an array field, or undefined for non-arrays. */
function elementKindOf(schema: z.ZodTypeAny): FieldKind | undefined {
  const core = unwrap(schema);
  if (!(core instanceof z.ZodArray)) return undefined;
  const element = (core as unknown as { element?: z.ZodTypeAny }).element;
  return element ? kindOf(element) : "string";
}

/** Describe every field of an argument shape, in declaration order. */
export function describeFields(shape: ArgShape): FieldInfo[] {
  return Object.entries(shape).map(([name, schema]) => {
    const typed = schema as z.ZodTypeAny;
    const kind = kindOf(typed);
    const info: FieldInfo = {
      name,
      flag: camelToKebab(name),
      kind,
      required: !typed.safeParse(undefined).success,
    };
    if (kind === "array") info.elementKind = elementKindOf(typed);
    const description = (typed as { description?: string }).description;
    if (description) info.description = description;
    return info;
  });
}
