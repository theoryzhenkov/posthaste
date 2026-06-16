import { readdirSync, readFileSync, statSync } from 'node:fs'
import { join, relative } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = fileURLToPath(new URL('..', import.meta.url))
const sourceRoot = join(root, 'src')
const allowedHttpBridge = 'src/runtime/httpAdapter.ts'

const migratedHttpSymbols = new Set([
  'authHeaders',
  'buildAccountLogoUrl',
  'buildEventsUrl',
  'buildMessageAttachmentUrl',
  'fetchConversation',
  'fetchMailboxes',
  'fetchMessage',
  'fetchSearchMessages',
  'fetchSmartMailboxMessages',
  'fetchSmartMailboxes',
  'fetchSourceMessages',
  'performMessageCommand',
  'read',
])

interface Violation {
  line: number
  symbol: string
}

let failed = false

function visit(dir: string, files: string[] = []): string[] {
  for (const entry of readdirSync(dir)) {
    const path = join(dir, entry)
    const stat = statSync(path)
    if (stat.isDirectory()) {
      visit(path, files)
    } else if (/\.(ts|tsx)$/.test(entry)) {
      files.push(path)
    }
  }
  return files
}

function lineNumberAt(source: string, offset: number): number {
  return source.slice(0, offset).split('\n').length
}

function importedNames(importList: string): string[] {
  return importList
    .split(',')
    .map((item) => item.trim())
    .filter(Boolean)
    .map((item) => item.split(/\s+as\s+/)[0]?.trim() ?? '')
    .filter(Boolean)
}

function isApiClientModule(modulePath: string): boolean {
  return /(?:^|\/)api\/client(?:\/|$)/.test(modulePath)
}

function importViolations(source: string): Violation[] {
  const violations: Violation[] = []
  const namedImportPattern =
    /import\s*\{([\s\S]*?)\}\s*from\s*['"]([^'"]*api\/client(?:\/[^'"]*)?)['"]/g
  const namespaceImportPattern =
    /import\s+\*\s+as\s+\w+\s+from\s*['"]([^'"]*api\/client(?:\/[^'"]*)?)['"]/g
  const defaultImportPattern =
    /import\s+\w+\s+from\s*['"]([^'"]*api\/client(?:\/[^'"]*)?)['"]/g

  for (const match of source.matchAll(namedImportPattern)) {
    const importList = match[1] ?? ''
    const modulePath = match[2] ?? ''
    if (!isApiClientModule(modulePath)) {
      continue
    }
    for (const symbol of importedNames(importList)) {
      if (!migratedHttpSymbols.has(symbol)) {
        continue
      }
      violations.push({ line: lineNumberAt(source, match.index ?? 0), symbol })
    }
  }

  for (const match of source.matchAll(namespaceImportPattern)) {
    violations.push({
      line: lineNumberAt(source, match.index ?? 0),
      symbol: 'api/client namespace import',
    })
  }

  for (const match of source.matchAll(defaultImportPattern)) {
    violations.push({
      line: lineNumberAt(source, match.index ?? 0),
      symbol: 'api/client default import',
    })
  }

  return violations
}

for (const file of visit(sourceRoot)) {
  const rel = relative(root, file).replaceAll('\\', '/')
  if (rel === allowedHttpBridge || rel.startsWith('src/api/client')) {
    continue
  }

  const violations = importViolations(readFileSync(file, 'utf8'))
  for (const violation of violations) {
    failed = true
    console.error(
      `${rel}:${violation.line}: ${violation.symbol} is a migrated mail runtime operation. ` +
        `Import it through src/runtime/adapter.ts; only ${allowedHttpBridge} may wrap api/client transport.`,
    )
  }
}

if (failed) {
  process.exit(1)
}
