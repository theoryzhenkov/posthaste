/**
 * Behavior-preservation tests for the Phase B connection layer.
 *
 * The keystone safety property: with `__POSTHASTE_PORT__`/`__POSTHASTE_TOKEN__`
 * injected (the bundled build), the dynamic resolution must produce the SAME
 * baseUrl/token the old module-load `const BASE_URL`/`AUTH_TOKEN` did, and the
 * client URL/header builders must be byte-for-byte identical to today. This
 * proves the 44 API consumers are unaffected by the refactor.
 *
 * @spec docs/eph/DESIGN-L1-deployment-modes#connection-profiles
 */
import { afterEach, beforeEach, describe, expect, it } from 'bun:test'

// Establish a window with the injected globals BEFORE importing the modules
// that read them at load time (runtime.ts seeds the active connection from the
// injection synchronously).
const PORT = 4815
const TOKEN = 'embedded-token-abc'

function installInjectedWindow(): void {
  ;(globalThis as Record<string, unknown>).window = {
    __POSTHASTE_PORT__: PORT,
    __POSTHASTE_TOKEN__: TOKEN,
  }
}

beforeEach(() => {
  installInjectedWindow()
})

afterEach(() => {
  delete (globalThis as Record<string, unknown>).window
})

describe('embedded resolution matches the legacy frozen consts', () => {
  it('resolves the same baseUrl + token the old consts produced', async () => {
    const { resolveActiveConnection } =
      await import('../src/connection/resolve')
    const { resetClientStoreForTesting } =
      await import('../src/connection/store')
    // Force the default (embedded-active) store without a real backend.
    resetClientStoreForTesting()

    const resolution = await resolveActiveConnection()
    expect(resolution.status).toBe('connected')
    if (resolution.status !== 'connected') {
      throw new Error('expected connected')
    }
    // Old behavior: `http://127.0.0.1:<port>/v1` from `__POSTHASTE_PORT__`,
    // token from `__POSTHASTE_TOKEN__`.
    expect(resolution.connection.baseUrl).toBe(`http://127.0.0.1:${PORT}/v1`)
    expect(resolution.connection.token).toBe(TOKEN)
    expect(resolution.connection.hostHeader).toBeUndefined()
  })

  it('seeds the runtime holder synchronously to the embedded default', async () => {
    const { getActiveConnection } = await import('../src/connection/runtime')
    const conn = getActiveConnection()
    expect(conn.baseUrl).toBe(`http://127.0.0.1:${PORT}/v1`)
    expect(conn.token).toBe(TOKEN)
  })
})

describe('client builders read the active connection identically to today', () => {
  it('builds the same SSE events URL with the access_token query param', async () => {
    const { buildEventsUrl } = await import('../src/api/client')
    // Old: `${BASE_URL}/events?...&access_token=${AUTH_TOKEN}`.
    expect(buildEventsUrl()).toBe(
      `http://127.0.0.1:${PORT}/v1/events?access_token=${TOKEN}`,
    )
    expect(buildEventsUrl({ accountId: 'acct-1', afterSeq: 7 })).toBe(
      `http://127.0.0.1:${PORT}/v1/events?accountId=acct-1&afterSeq=7&access_token=${TOKEN}`,
    )
  })

  it('builds the same attachment + logo URLs', async () => {
    const { buildMessageAttachmentUrl, buildAccountLogoUrl } =
      await import('../src/api/client')
    expect(
      buildMessageAttachmentUrl('src 1', 'msg/2', 'att:3', { download: true }),
    ).toBe(
      `http://127.0.0.1:${PORT}/v1/sources/src%201/messages/msg%2F2/attachments/att%3A3?download=1`,
    )
    expect(buildAccountLogoUrl('logo 1')).toBe(
      `http://127.0.0.1:${PORT}/v1/account-assets/logos/logo%201`,
    )
  })
})
