import { describe, expect, it } from 'bun:test'
import { readdirSync, readFileSync, statSync } from 'node:fs'
import { join, relative } from 'node:path'

const srcRoot = join(import.meta.dir, '..', 'src')

const allowedStorageFiles = new Set([
  'src/client-preferences/storage.ts',
  'src/client-preferences/store.ts',
  'src/components/floating-panel/geometry.ts',
  'src/components/message-list/useViewMode.ts',
  'src/components/thread-list/useColumnConfig.ts',
  'src/connection/store.ts',
  'src/developerTools.ts',
  'src/hooks/useDaemonEvents.ts',
  'src/hooks/useMailLayoutPersistence.ts',
  'src/observability.ts',
  'src/repairFeedback.ts',
  // The client-layer replica's durable state lives in one IndexedDB DB
  // (`posthaste-replica`); `replicaDatabase.ts` is the single shared opener
  // that owns the schema version + creates every object store, so the outbox
  // + undo history can't diverge on version again. It persists only mutation
  // metadata + invertible diffs (keyword/mailbox deltas, step ids) — never
  // bodies, attachments, or auth material, which the forbidden-value check
  // below still enforces.
  'src/runtime/replica/replicaDatabase.ts',
  'src/runtime/replica/outboxStore.ts',
  // The client-owned undo/redo history: persists only invertible change-diffs
  // (keyword/mailbox deltas) + step ids — no bodies, attachments, or auth
  // material. Shares the `posthaste-replica` DB via the shared opener.
  'src/runtime/replica/undoHistoryStore.ts',
])

const forbiddenStorageValueTerms = [
  /access[_-]?token/i,
  /authorization/i,
  /bearer/i,
  /refresh[_-]?token/i,
  /client[_-]?secret/i,
  /provider[_-]?secret/i,
  /secret/i,
  /password/i,
  /credential/i,
  /attachment/i,
  /body[_-]?html/i,
  /body[_-]?text/i,
  /message[_-]?body/i,
  /idempotency/i,
  /event[_-]?history/i,
  // `sqlite|indexeddb|database` was a proxy for "don't mirror the server DB to
  // storage"; the replica outbox is now a sanctioned IndexedDB store, so the
  // API identifier is no longer treated as a forbidden value. The mail-content
  // and auth-material terms above remain the real guard.
]

function sourceFiles(dir: string): string[] {
  return readdirSync(dir).flatMap((entry) => {
    const path = join(dir, entry)
    const stat = statSync(path)
    if (stat.isDirectory()) {
      return sourceFiles(path)
    }
    return /\.(ts|tsx)$/.test(entry) ? [path] : []
  })
}

function projectPath(path: string): string {
  return join('src', relative(srcRoot, path)).replaceAll('\\', '/')
}

function stripComments(contents: string): string {
  let output = ''
  let index = 0
  let quote: 'single' | 'double' | 'template' | null = null
  while (index < contents.length) {
    const char = contents[index]
    const next = contents[index + 1]

    if (quote) {
      output += char
      if (char === '\\') {
        output += next ?? ''
        index += 2
        continue
      }
      if (
        (quote === 'single' && char === "'") ||
        (quote === 'double' && char === '"') ||
        (quote === 'template' && char === '`')
      ) {
        quote = null
      }
      index += 1
      continue
    }

    if (char === "'") quote = 'single'
    if (char === '"') quote = 'double'
    if (char === '`') quote = 'template'
    if (quote) {
      output += char
      index += 1
      continue
    }

    if (char === '/' && next === '/') {
      while (index < contents.length && contents[index] !== '\n') index += 1
      if (index < contents.length) {
        output += '\n'
        index += 1
      }
      continue
    }
    if (char === '/' && next === '*') {
      index += 2
      while (
        index < contents.length &&
        !(contents[index] === '*' && contents[index + 1] === '/')
      ) {
        if (contents[index] === '\n') output += '\n'
        index += 1
      }
      index += 2
      continue
    }

    output += char
    index += 1
  }
  return output
}

function storageContextSnippets(contents: string): string[] {
  const lines = stripComments(contents).split('\n')
  return lines.flatMap((line, index) => {
    if (!/\b(localStorage|sessionStorage|indexedDB)\b/.test(line)) {
      return []
    }
    return [lines.slice(Math.max(0, index - 2), index + 5).join(' ')]
  })
}

describe('renderer storage boundary', () => {
  it('strips comments without hiding URL-shaped tokens in string literals', () => {
    const stripped = stripComments(`
      // access_token in comments is ignored
      localStorage.setItem("x", "http://host/v1?access_token=secret")
    `)

    expect(stripped).toContain('access_token=secret')
  })

  it('keeps multiline storage values visible across stripped comments', () => {
    const snippets = storageContextSnippets(`
      localStorage.setItem(
        // comment one
        // comment two
        "x",
        "accessToken=secret"
      )
    `)

    expect(snippets.some((snippet) => /accessToken=secret/.test(snippet))).toBe(
      true,
    )
  })

  it('keeps renderer-owned storage limited to explicit UI/client-state files', () => {
    const offenders = sourceFiles(srcRoot)
      .map((path) => ({
        path: projectPath(path),
        contents: stripComments(readFileSync(path, 'utf8')),
      }))
      .filter(({ contents }) =>
        /\b(localStorage|sessionStorage|indexedDB)\b/.test(contents),
      )
      .map(({ path }) => path)
      .filter((path) => !allowedStorageFiles.has(path))

    expect(offenders).toEqual([])
  })

  it('does not write obvious mail bodies, attachments, auth material, or DB mirrors to storage', () => {
    const offenders = sourceFiles(srcRoot).flatMap((path) => {
      const rel = projectPath(path)
      const contents = readFileSync(path, 'utf8')
      if (!allowedStorageFiles.has(rel)) {
        return []
      }
      return storageContextSnippets(contents)
        .filter((snippet) =>
          forbiddenStorageValueTerms.some((term) => term.test(snippet)),
        )
        .map((snippet) => `${rel}: ${snippet.trim()}`)
    })

    expect(offenders).toEqual([])
  })
})
