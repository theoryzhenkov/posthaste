export interface OperationContext {
  operationId: string
  operationKind: string
  operationSource: string
  sessionId: string
}

export interface RequestContext extends OperationContext {
  requestId: string
}

const SESSION_STORAGE_KEY = 'posthaste:observability-session-id'

function randomId(prefix: string): string {
  const random =
    typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function'
      ? crypto.randomUUID()
      : Math.random().toString(36).slice(2)
  return `${prefix}_${random}`
}

export function currentObservabilitySessionId(): string {
  try {
    const stored = window.sessionStorage.getItem(SESSION_STORAGE_KEY)
    if (stored) {
      return stored
    }
    const next = randomId('session')
    window.sessionStorage.setItem(SESSION_STORAGE_KEY, next)
    return next
  } catch {
    return randomId('session')
  }
}

export function createOperationContext(
  operationKind: string,
  operationSource: string,
): OperationContext {
  return {
    operationId: randomId('op'),
    operationKind,
    operationSource,
    sessionId: currentObservabilitySessionId(),
  }
}

export function createRequestContext(
  operation: OperationContext,
): RequestContext {
  return {
    ...operation,
    requestId: randomId('req'),
  }
}

export function observabilityHeaders(context: RequestContext): HeadersInit {
  return {
    'X-PostHaste-Request-Id': context.requestId,
    'X-PostHaste-Operation-Id': context.operationId,
    'X-PostHaste-Operation-Kind': context.operationKind,
    'X-PostHaste-Operation-Source': context.operationSource,
    'X-PostHaste-Session-Id': context.sessionId,
  }
}
