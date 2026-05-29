/**
 * Drift guard: the committed `src/api/schema.gen.ts` must match what
 * `openapi-typescript` produces from the committed `openapi.json`. This is the
 * web-side analogue of the Rust `openapi_contract` test — together they keep
 * backend handlers, the spec, and the generated TS in lockstep.
 *
 * @spec docs/L1-api#openapi-contract
 */
import { execFileSync } from 'node:child_process'
import { mkdtempSync, readFileSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = fileURLToPath(new URL('..', import.meta.url))
const spec = join(root, '../../openapi.json')
const committed = join(root, 'src/api/schema.gen.ts')
const dir = mkdtempSync(join(tmpdir(), 'ph-openapi-'))
const fresh = join(dir, 'schema.gen.ts')

try {
  execFileSync('bunx', ['openapi-typescript', spec, '-o', fresh], {
    stdio: 'pipe',
  })
  if (readFileSync(committed, 'utf8') !== readFileSync(fresh, 'utf8')) {
    console.error(
      'src/api/schema.gen.ts is out of date with openapi.json.\n' +
        'Regenerate and commit it: `bun run api:generate`',
    )
    process.exit(1)
  }
} finally {
  rmSync(dir, { recursive: true, force: true })
}
