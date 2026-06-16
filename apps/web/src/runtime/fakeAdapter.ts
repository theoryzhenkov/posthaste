import type { MessageCommandResult } from '../api/types'

import type { RuntimeAdapter, RuntimeMessageCommandRequest } from './types'

type QueuedMessageCommandOutcome =
  | { kind: 'resolve'; value: MessageCommandResult }
  | { kind: 'reject'; error: Error }

const defaultMessageCommandResult: MessageCommandResult = {
  detail: null,
  events: [],
}

export interface FakeRuntimeAdapter extends RuntimeAdapter {
  readonly messageCommandCalls: RuntimeMessageCommandRequest[]
  queueMessageCommandResult(result: MessageCommandResult): void
  queueMessageCommandError(error: Error): void
  reset(): void
}

/** Fake runtime adapter for renderer tests. Does not start or contact a backend. */
export function createFakeRuntimeAdapter(input?: {
  defaultMessageCommandResult?: MessageCommandResult
}): FakeRuntimeAdapter {
  const messageCommandCalls: RuntimeMessageCommandRequest[] = []
  const queuedMessageCommandOutcomes: QueuedMessageCommandOutcome[] = []
  const fallback =
    input?.defaultMessageCommandResult ?? defaultMessageCommandResult

  return {
    messageCommandCalls,
    queueMessageCommandResult(result) {
      queuedMessageCommandOutcomes.push({ kind: 'resolve', value: result })
    },
    queueMessageCommandError(error) {
      queuedMessageCommandOutcomes.push({ kind: 'reject', error })
    },
    reset() {
      messageCommandCalls.length = 0
      queuedMessageCommandOutcomes.length = 0
    },
    async runMessageCommand(request) {
      messageCommandCalls.push({ ...request })
      const next = queuedMessageCommandOutcomes.shift()
      if (!next) {
        return fallback
      }
      if (next.kind === 'reject') {
        throw next.error
      }
      return next.value
    },
  }
}
