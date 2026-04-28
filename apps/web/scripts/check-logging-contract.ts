import { readdirSync, readFileSync, statSync } from 'node:fs'
import { join, relative } from 'node:path'

const root = new URL('..', import.meta.url).pathname
const sourceRoot = join(root, 'src')
const allowedPinoImport = 'src/logger.ts'
const allowedEventLiteral = 'src/logEvents.ts'
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

for (const file of visit(sourceRoot)) {
  const rel = relative(root, file)
  const source = readFileSync(file, 'utf8')

  if (rel !== allowedPinoImport && /from ['"]pino['"]/.test(source)) {
    console.error(`${rel}: import pino only inside src/logger.ts`)
    failed = true
  }

  if (rel !== allowedEventLiteral && /\bevent:\s*['"]/.test(source)) {
    console.error(
      `${rel}: use LOG_EVENTS constants instead of event string literals`,
    )
    failed = true
  }

  if (rel !== allowedPinoImport && /\blogger\.child\(/.test(source)) {
    console.error(`${rel}: create loggers through src/logger.ts typedLogger`)
    failed = true
  }
}

if (failed) {
  process.exit(1)
}
