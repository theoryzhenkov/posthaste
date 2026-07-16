import { describe, expect, it } from 'bun:test'
import { readdirSync, readFileSync, statSync } from 'node:fs'
import { join, relative } from 'node:path'

const srcRoot = join(import.meta.dir, '..', 'src')

// The message body + attachment URL builders may only appear in their own
// definitions and the single adapter resolver (`resourceUrl`). Everything else
// must fetch resource bytes through `runtime/resources.ts`, so body and
// attachments stay one lazy-resource pipeline and no component grows a bespoke
// fetch that bypasses the shared transport/policy.
const allowedResourceUrlFiles = new Set([
  'src/api/client/urls.ts', // builder definitions
  'src/runtime/httpAdapter.ts', // the single resourceUrl() resolver
])

function sourceFiles(dir: string): string[] {
  return readdirSync(dir).flatMap((entry) => {
    const path = join(dir, entry)
    return statSync(path).isDirectory()
      ? sourceFiles(path)
      : /\.(ts|tsx)$/.test(entry)
        ? [path]
        : []
  })
}

function projectPath(path: string): string {
  return join('src', relative(srcRoot, path)).replaceAll('\\', '/')
}

describe('lazy resource boundary', () => {
  it('builds message resource URLs only in the shared resource layer', () => {
    const offenders = sourceFiles(srcRoot)
      .filter((path) =>
        /buildMessage(Body|Attachment)Url\b/.test(readFileSync(path, 'utf8')),
      )
      .map(projectPath)
      .filter((path) => !allowedResourceUrlFiles.has(path))

    expect(offenders).toEqual([])
  })

  it('fetches resource bytes only through runtime/resources.ts', () => {
    const offenders = sourceFiles(srcRoot)
      .filter((path) =>
        /\.fetchResourceBlob\b/.test(readFileSync(path, 'utf8')),
      )
      .map(projectPath)
      // The adapter implements it; runtime/resources.ts is the one caller.
      .filter(
        (path) =>
          path !== 'src/runtime/resources.ts' &&
          path !== 'src/runtime/httpAdapter.ts' &&
          path !== 'src/runtime/adapter.ts' &&
          path !== 'src/runtime/types.ts',
      )

    expect(offenders).toEqual([])
  })
})
