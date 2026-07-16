/**
 * Regression pins for the v0.5.0 field bug "message.moveToRole did not return
 * a message command result": the wire contract says a confirmed mail-command
 * receipt always carries `output.events` as an array (empty ok, never absent),
 * and the server now guarantees it — but the client stays tolerant (defense in
 * depth): an otherwise-valid output OBJECT with `events` merely absent is
 * treated as "no events" instead of throwing, while a receipt whose output is
 * genuinely absent or mis-shaped still throws, and a failed receipt surfaces
 * its error message.
 */
import { describe, expect, it } from 'bun:test'

import type { MessageCommandResult } from '../src/api/types'
import { confirmedMessageCommandResult } from '../src/runtime/mutations'
import type { RuntimeMutationReceipt } from '../src/runtime/types'

function receipt(
  overrides: Partial<RuntimeMutationReceipt>,
): RuntimeMutationReceipt {
  return {
    runtimeMutationId: 'mutation-1',
    clientMutationId: 'client-1',
    name: 'message.moveToRole',
    state: 'confirmed',
    error: null,
    ...overrides,
  }
}

describe('confirmedMessageCommandResult', () => {
  it('returns the output when it carries an events array', () => {
    const events = [
      { topic: 'message.updated' },
    ] as unknown as MessageCommandResult['events']
    const result = confirmedMessageCommandResult(
      receipt({ output: { events } }),
    )
    expect(result.events).toEqual(events)
  })

  it('returns an empty events array when the output carries none', () => {
    const result = confirmedMessageCommandResult(
      receipt({ output: { events: [] } }),
    )
    expect(result.events).toEqual([])
  })

  it('tolerates a valid output object whose events field is absent', () => {
    // Defense in depth: a peer that drops the (contractually always-present)
    // `events` array must not crash the caller — it only loses the bundled
    // count-reconciliation echo.
    const result = confirmedMessageCommandResult(receipt({ output: {} }))
    expect(result.events).toEqual([])
  })

  it('still rejects a receipt whose output is null (the field-bug shape)', () => {
    // The pre-fix server answered an in-flight duplicate with a non-failed
    // receipt whose output was null. That is not a command result: throwing is
    // correct — the server-side fix keeps the shape from occurring at all.
    expect(() =>
      confirmedMessageCommandResult(receipt({ output: null })),
    ).toThrow('message.moveToRole did not return a message command result')
  })

  it('still rejects a receipt whose output is absent or mis-shaped', () => {
    expect(() => confirmedMessageCommandResult(receipt({}))).toThrow(
      'message.moveToRole did not return a message command result',
    )
    expect(() =>
      confirmedMessageCommandResult(receipt({ output: 'garbage' })),
    ).toThrow('message.moveToRole did not return a message command result')
    expect(() =>
      confirmedMessageCommandResult(receipt({ output: { events: 'nope' } })),
    ).toThrow('message.moveToRole did not return a message command result')
  })

  it('surfaces the error message of a failed receipt', () => {
    expect(() =>
      confirmedMessageCommandResult(
        receipt({
          state: 'failed',
          error: {
            code: 'conflict',
            message:
              'a mutation with this client mutation id is still in flight; retry',
            terminality: 'transient',
            correlationId: null,
            details: null,
          },
        }),
      ),
    ).toThrow(
      'a mutation with this client mutation id is still in flight; retry',
    )
  })
})
