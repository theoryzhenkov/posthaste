import { afterEach, beforeEach, describe, expect, it } from 'bun:test'

import { downloadAuthedResource } from '../src/lib/downloadAuthedResource'
import { setupDomEnvironment } from './dom-env'

setupDomEnvironment()

// Capture the save: the object URL created, the anchor click (href + download
// filename), and the eventual revoke. The helper triggers a save by clicking a
// transient <a download>, so we stub fetch + the object-URL globals and wrap the
// anchor's click to record what would have been downloaded.
let created: string[] = []
let revoked: string[] = []
let clicks: Array<{ href: string; download: string }> = []
let counter = 0

const originalFetch = globalThis.fetch
const originalCreate = URL.createObjectURL
const originalRevoke = URL.revokeObjectURL
let originalCreateElement: typeof document.createElement

function stubFetch(respond: () => Response): void {
  globalThis.fetch = (async () => respond()) as typeof fetch
}

const flushMacrotask = () => new Promise((resolve) => setTimeout(resolve, 5))

beforeEach(() => {
  created = []
  revoked = []
  clicks = []
  counter = 0
  URL.createObjectURL = (() => {
    const url = `blob:mock/${(counter += 1)}`
    created.push(url)
    return url
  }) as typeof URL.createObjectURL
  URL.revokeObjectURL = ((url: string) => {
    revoked.push(url)
  }) as typeof URL.revokeObjectURL
  originalCreateElement = document.createElement.bind(document)
  document.createElement = ((tag: string) => {
    const element = originalCreateElement(tag)
    if (tag === 'a') {
      ;(element as HTMLAnchorElement).click = () => {
        const anchor = element as HTMLAnchorElement
        clicks.push({
          href: anchor.href,
          download: anchor.getAttribute('download') ?? '',
        })
      }
    }
    return element
  }) as typeof document.createElement
})

afterEach(() => {
  globalThis.fetch = originalFetch
  URL.createObjectURL = originalCreate
  URL.revokeObjectURL = originalRevoke
  document.createElement = originalCreateElement
})

describe('downloadAuthedResource', () => {
  it('fetches, clicks a download anchor, and revokes the object URL', async () => {
    stubFetch(() => new Response('bytes', { status: 200 }))
    await downloadAuthedResource(
      '/v1/sources/a/messages/m/attachments/x',
      'doc.pdf',
    )

    expect(created).toEqual(['blob:mock/1'])
    expect(clicks).toEqual([{ href: 'blob:mock/1', download: 'doc.pdf' }])

    // Revoke is deferred to the next tick so the browser can read the blob.
    expect(revoked).toEqual([])
    await flushMacrotask()
    expect(revoked).toEqual(['blob:mock/1'])
  })

  it('throws on a non-ok response and never creates an object URL', async () => {
    stubFetch(() => new Response('', { status: 403 }))
    await expect(
      downloadAuthedResource(
        '/v1/sources/a/messages/m/attachments/x',
        'doc.pdf',
      ),
    ).rejects.toThrow('download failed with 403')
    expect(created).toEqual([])
    expect(clicks).toEqual([])
  })
})
