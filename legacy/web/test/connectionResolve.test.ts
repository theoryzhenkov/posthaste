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
import { afterAll, beforeAll, beforeEach, describe, expect, it } from 'bun:test'

// Use an explicit injection override instead of mutating `window`: bun runs test
// files concurrently, and DOM tests may register/unregister their own window.
const PORT = 4815
const TOKEN = 'embedded-token-abc'

beforeAll(async () => {
  const { setInjectedRuntimeForTesting } =
    await import('../src/connection/injected')
  setInjectedRuntimeForTesting({
    port: PORT,
    runtimeMode: 'loopback',
    token: TOKEN,
  })
})

beforeEach(async () => {
  const { resetActiveConnectionForTesting } =
    await import('../src/connection/runtime')
  resetActiveConnectionForTesting()
})

afterAll(async () => {
  const { setInjectedRuntimeForTesting } =
    await import('../src/connection/injected')
  setInjectedRuntimeForTesting(undefined)
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

  it('exposes injected runtime mode for adapter selection', async () => {
    const { injectedRuntimeMode } = await import('../src/connection/injected')
    expect(injectedRuntimeMode()).toBe('loopback')
  })

  it('seeds the runtime holder synchronously to the embedded default', async () => {
    const { getActiveConnection } = await import('../src/connection/runtime')
    const conn = getActiveConnection()
    expect(conn.baseUrl).toBe(`http://127.0.0.1:${PORT}/v1`)
    expect(conn.token).toBe(TOKEN)
  })
})

describe('client builders read the active connection without a URL token', () => {
  it('builds attachment + logo URLs with no token (loaded via authed blob fetch)', async () => {
    const { buildMessageAttachmentUrl, buildAccountLogoUrl } =
      await import('../src/api/client')
    expect(buildMessageAttachmentUrl('src 1', 'msg/2', 'att:3')).toBe(
      `http://127.0.0.1:${PORT}/v1/sources/src%201/messages/msg%2F2/attachments/att%3A3`,
    )
    expect(buildAccountLogoUrl('logo 1')).toBe(
      `http://127.0.0.1:${PORT}/v1/account-assets/logos/logo%201`,
    )
  })

  it('exposes the bearer token via authHeaders for header-authed fetches', async () => {
    const { authHeaders } = await import('../src/api/client')
    expect(authHeaders()).toEqual({ Authorization: `Bearer ${TOKEN}` })
  })
})
