// The shared operation registry: one list, two renderings. The CLI renders
// each entry as a subcommand; the MCP server registers each as a stdio tool.
// Adding an operation here is the whole job — both front-ends pick it up.

import { readOperations } from "./read.js";
import { writeOperations } from "./commands.js";

export type { ArgShape, CliBinding, Operation } from "./types.js";
export { defineOperation } from "./types.js";

export const operations = [...readOperations, ...writeOperations];
