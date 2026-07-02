import type { z } from "zod";

import type { Connection } from "../client.js";

/**
 * A Zod *raw shape* (the per-field schema map). This is exactly what the MCP
 * SDK's `registerTool` accepts as `inputSchema`, and what the CLI introspects to
 * derive flags — one schema, both front-ends.
 */
export type ArgShape = z.ZodRawShape;

/** How an operation renders as a `posthastectl` subcommand. */
export interface CliBinding {
  /**
   * Command path, e.g. `["messages", "search"]` →
   * `posthastectl messages search`.
   */
  path: string[];
  /**
   * Optional single positional argument: the field a bare first token maps to,
   * so `messages search "hello"` fills `--q`. Everything else is flags / `--json`.
   */
  primary?: string;
}

/**
 * One API operation, lifted out of any front-end. The MCP server and the CLI are
 * two *renderings* of this same descriptor (docs/eph/RFC-L2-scripting.md §7,
 * the ladder): MCP registers it as a tool, the CLI renders it as a subcommand.
 * Neither front-end can drift from the other or from the `/v1` contract,
 * because both consume the same `argSchema` + `handler`.
 */
export interface Operation {
  /** Stable MCP tool name (unchanged from the original `tools/` registration). */
  mcpName: string;
  /** Short human title (MCP tool title). */
  title: string;
  /** One-line description — doubles as MCP tool description and CLI help. */
  description: string;
  /**
   * Whether the operation mutates server state. Drives the MCP `readOnlyHint`
   * annotation and CLI presentation (read vs command). Mirrors the original
   * read-vs-command tool split.
   */
  mutates: boolean;
  /** CLI rendering. */
  cli: CliBinding;
  /** Per-field Zod shape; validates CLI flags AND the MCP tool input. */
  argSchema: ArgShape;
  /** The HTTP call against the daemon `/v1` API. */
  handler: (
    conn: Connection,
    args: Record<string, unknown>,
  ) => Promise<unknown>;
}

/**
 * Define an operation with definition-site type inference: the `handler` sees
 * args typed from `argSchema`, while the result erases to the uniform
 * [`Operation`] so a heterogeneous registry array stays well-typed.
 */
export function defineOperation<Shape extends ArgShape>(op: {
  mcpName: string;
  title: string;
  description: string;
  mutates: boolean;
  cli: { path: string[]; primary?: Extract<keyof Shape, string> };
  argSchema: Shape;
  handler: (
    conn: Connection,
    args: z.infer<z.ZodObject<Shape>>,
  ) => Promise<unknown>;
}): Operation {
  return op as unknown as Operation;
}
