/**
 * Drift guard: the committed `src/api/querySchema.gen.ts` must match what
 * `scripts/gen-query-schema.ts` produces from the committed `query-schema.json`.
 * This is the query-schema analogue of `check-event-topics.ts`: together with the
 * Rust `query_schema_contract` test (artifact vs Rust schema) it keeps the store
 * compiler, the JSON artifact, and the generated web registry in lockstep — a
 * Rust-side field/operator change fails the web build here until it is
 * regenerated.
 *
 * @spec docs/eph/RFC-L2-query-schema.md#d4--one-canonical-field-schema
 */
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'

import { renderQuerySchemaModule } from './gen-query-schema.ts'

const artifact = fileURLToPath(
  new URL('../../../query-schema.json', import.meta.url),
)
const committed = fileURLToPath(
  new URL('../src/api/querySchema.gen.ts', import.meta.url),
)

const fresh = renderQuerySchemaModule(readFileSync(artifact, 'utf8'))
if (readFileSync(committed, 'utf8') !== fresh) {
  console.error(
    'src/api/querySchema.gen.ts is out of date with query-schema.json.\n' +
      'Regenerate and commit it: `bun run query-schema:generate`',
  )
  process.exit(1)
}
