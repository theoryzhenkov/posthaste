/**
 * Drift guard: the committed `src/api/events.gen.ts` must match what
 * `scripts/gen-event-topics.ts` produces from the committed `asyncapi.json`.
 * This is the event-side analogue of `check-openapi-types.ts`: together they keep
 * the backend event contract, the AsyncAPI spec, and the generated client mirror
 * (including the exhaustive domain-cache handler registry) in lockstep. A
 * server-side topic addition fails the client build here until it is handled.
 *
 * @spec docs/L1-api#sse-event-stream
 */
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'

import { renderEventTopicsModule } from './gen-event-topics.ts'

const asyncapi = fileURLToPath(
  new URL('../../../asyncapi.json', import.meta.url),
)
const committed = fileURLToPath(
  new URL('../src/api/events.gen.ts', import.meta.url),
)

const fresh = renderEventTopicsModule(readFileSync(asyncapi, 'utf8'))
if (readFileSync(committed, 'utf8') !== fresh) {
  console.error(
    'src/api/events.gen.ts is out of date with asyncapi.json.\n' +
      'Regenerate and commit it: `bun run events:generate`',
  )
  process.exit(1)
}
