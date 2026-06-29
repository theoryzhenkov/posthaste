import { commandOperations } from "./commands.js";
import { readOperations } from "./read.js";
import type { Operation } from "./types.js";

export type { ArgShape, CliBinding, Operation } from "./types.js";
export { defineOperation } from "./types.js";

/**
 * The full operation registry: the single source of truth both front-ends
 * render. The MCP server (`index.ts`) registers each entry as a tool; the CLI
 * (`cli.ts`) renders each as a subcommand. Order here is the order tools/commands
 * are surfaced.
 */
export const operations: Operation[] = [
  ...readOperations,
  ...commandOperations,
];
