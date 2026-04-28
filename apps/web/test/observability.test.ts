import { describe, expect, it } from 'bun:test'

import {
  createRequestContext,
  observabilityHeaders,
  type OperationContext,
} from '../src/observability'

describe('observability correlation', () => {
  const operation: OperationContext = {
    operationId: 'op_123',
    operationKind: 'mail.search',
    operationSource: 'message-list',
    sessionId: 'session_456',
  }

  // spec: docs/L1-logging#request-correlation
  it('creates a request id for an existing operation context', () => {
    const context = createRequestContext(operation)

    expect(context.requestId).toStartWith('req_')
    expect(context.operationId).toBe(operation.operationId)
    expect(context.operationKind).toBe(operation.operationKind)
    expect(context.operationSource).toBe(operation.operationSource)
    expect(context.sessionId).toBe(operation.sessionId)
  })

  // spec: docs/L1-logging#operation-correlation
  it('maps request context to API correlation headers', () => {
    expect(
      observabilityHeaders({
        ...operation,
        requestId: 'req_789',
      }),
    ).toEqual({
      'X-PostHaste-Request-Id': 'req_789',
      'X-PostHaste-Operation-Id': 'op_123',
      'X-PostHaste-Operation-Kind': 'mail.search',
      'X-PostHaste-Operation-Source': 'message-list',
      'X-PostHaste-Session-Id': 'session_456',
    })
  })
})
