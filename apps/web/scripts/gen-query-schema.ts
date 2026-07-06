/**
 * Generator for `src/api/querySchema.gen.ts` — the client mirror of the canonical
 * mail-query field schema. Reads the committed `query-schema.json` artifact
 * (emitted from `posthaste_domain_model::query_schema_document` and drift-checked
 * by the Rust `query_schema_contract` test) and emits, per field, its coarse
 * value type + the exact set of allowed operators.
 *
 * This is the single source that kills the Rust↔TS operator/field drift (RFC-L2
 * D4): the store SQL compiler and this table both derive from the SAME Rust
 * schema, so the condition editor can no longer offer an operator the compiler
 * rejects. `fieldRegistry.ts` consumes this DATA and layers its own presentation
 * (widget/label) on top.
 *
 * Run with `bun run query-schema:generate`; `bun run query-schema:check` fails the
 * build when the committed output drifts from what this generator would produce.
 *
 * @spec docs/eph/RFC-L2-query-schema.md#d4--one-canonical-field-schema
 */
import { readFileSync, writeFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'

const root = fileURLToPath(new URL('..', import.meta.url))
const artifactPath = new URL('../../../query-schema.json', import.meta.url)
const outPath = new URL('../src/api/querySchema.gen.ts', import.meta.url)

interface SchemaEntry {
  field: string
  valueType: string
  operators: string[]
}

/** Parse + validate the `query-schema.json` contract artifact. */
export function readQuerySchema(artifactJson: string): SchemaEntry[] {
  const doc = JSON.parse(artifactJson) as { fields?: unknown }
  const fields = doc.fields
  if (!Array.isArray(fields)) {
    throw new Error('query-schema.json: `fields` must be an array')
  }
  return fields.map((raw, index) => {
    const entry = raw as Partial<SchemaEntry>
    if (
      typeof entry.field !== 'string' ||
      typeof entry.valueType !== 'string' ||
      !Array.isArray(entry.operators) ||
      entry.operators.some((op) => typeof op !== 'string')
    ) {
      throw new Error(
        `query-schema.json: malformed field entry at index ${index}`,
      )
    }
    return {
      field: entry.field,
      valueType: entry.valueType,
      operators: entry.operators as string[],
    }
  })
}

/** Render the full `querySchema.gen.ts` module source from the artifact. */
export function renderQuerySchemaModule(artifactJson: string): string {
  const entries = readQuerySchema(artifactJson)

  const valueTypes: string[] = []
  for (const { valueType } of entries) {
    if (!valueTypes.includes(valueType)) {
      valueTypes.push(valueType)
    }
  }
  const valueTypeUnion = valueTypes.map((t) => `'${t}'`).join(' | ')

  const rows = entries
    .map(({ field, valueType, operators }) => {
      const ops = operators.map((op) => `'${op}'`).join(', ')
      return `  ${field}: { valueType: '${valueType}', operators: [${ops}] },`
    })
    .join('\n')

  const allFields = entries.map(({ field }) => `  '${field}',`).join('\n')

  return `/**
 * This file was auto-generated from query-schema.json by
 * scripts/gen-query-schema.ts. Do not make direct changes to the file.
 *
 * Regenerate: \`bun run query-schema:generate\`. The committed copy is
 * drift-checked verbatim by \`bun run query-schema:check\`. The artifact itself is
 * emitted from the canonical Rust schema
 * (\`posthaste_domain_model::query_schema_document\`), so the field set, per-field
 * value type, and per-field operators here can never diverge from the store SQL
 * compiler.
 */
import type { SmartMailboxField, SmartMailboxOperator } from './types'

/** The coarse value-shape family of a query field (the Rust \`QueryValueType\`). */
export type QueryValueType = ${valueTypeUnion}

/** A field's canonical spec: its value type and the operators it accepts. */
export interface QueryFieldSchema {
  valueType: QueryValueType
  operators: readonly SmartMailboxOperator[]
}

/**
 * The canonical field -> { valueType, operators } table, generated from the
 * Rust schema. Presentation (widget + label) lives in \`fieldRegistry.ts\`; this
 * is only the drift-prone DATA the store compiler shares.
 */
export const QUERY_FIELD_SCHEMA: Record<SmartMailboxField, QueryFieldSchema> = {
${rows}
}

/** Every query field, in the schema's canonical declaration order. */
export const ALL_QUERY_FIELDS: readonly SmartMailboxField[] = [
${allFields}
]
`
}

if (import.meta.main) {
  const artifactJson = readFileSync(fileURLToPath(artifactPath), 'utf8')
  writeFileSync(fileURLToPath(outPath), renderQuerySchemaModule(artifactJson))
  console.log(`Wrote ${fileURLToPath(outPath).slice(root.length)}`)
}
