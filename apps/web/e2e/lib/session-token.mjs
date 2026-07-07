// Mint a write-capable session token from the daemon's discovery bootstrap.
//
// `daemon.json`'s `token` is the least-privilege bootstrap capability — exactly
// `{action = mint, read}` (RFC-L2-scripting §7 ruling 11). It can read and it
// can MINT, but it holds no write verb, so submitting any mutation with it
// (archive, tag, snooze, send…) is 403'd by the per-operation authorizer on
// `POST /v1/runtime/sessions/{id}/mutations` and by the write-gated command
// routes. E2E scripts therefore trade the bootstrap for a fresh, expiring,
// write-capable token via `POST /v1/auth/tokens` (the same flow
// `posthastectl token mint` uses) and inject THAT into the page.
export async function mintSessionToken(daemon) {
  const res = await fetch(`http://127.0.0.1:${daemon.port}/v1/auth/tokens`, {
    method: 'POST',
    headers: {
      Authorization: `Bearer ${daemon.token}`,
      'Content-Type': 'application/json',
    },
    body: JSON.stringify({
      // Everything an e2e run may exercise; deliberately no `manage`/`mint`.
      actions: ['read', 'tag', 'move', 'delete', 'send'],
      expiresInSeconds: 3600,
    }),
  })
  if (!res.ok) {
    throw new Error(
      `minting the e2e session token failed: ${res.status} ${await res.text()}`,
    )
  }
  const { token } = await res.json()
  return token
}
