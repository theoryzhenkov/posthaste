import type { z } from "zod";

import type { Connection } from "../core/connection.js";

/**
 * A Zod *raw shape* (the per-field schema map). This is exactly what the MCP
 * SDK's `registerTool` accepts as `inputSchema`, and what the CLI introspects
 * to derive flags — one schema, both front-ends.
 */
export type ArgShape = z.ZodRawShape;

/** How an operation renders as a `posthastectl` subcommand. */
export interface CliBinding {
  /** Command path, e.g. `["messages", "search"]`. */
  path: string[];
  /**
   * Optional single positional argument: the field a bare token maps to, so
   * `posthastectl messages search "hello"` fills `--query`.
   */
  primary?: string;
}

/**
 * One API operation, lifted out of any front-end. The MCP server and the CLI
 * are two renderings of this descriptor: MCP registers it as a tool, the CLI
 * renders it as a subcommand. Neither can drift from the other or from the
 * generated wire contract, because both consume the same `argSchema` +
 * `handler`.
 */
export interface Operation {
  /** Stable snake_case MCP tool name. */
  mcpName: string;
  /** Short human title (MCP tool title). */
  title: string;
  /** One-line description — MCP tool description and CLI help. */
  description: string;
  /** Whether the operation mutates state (drives MCP `readOnlyHint`). */
  mutates: boolean;
  /** CLI rendering. */
  cli: CliBinding;
  /** Per-field Zod shape; validates CLI flags AND the MCP tool input. */
  argSchema: ArgShape;
  /** The typed call against the backend's query/command surface. */
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
