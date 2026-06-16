import type { MessageCommandResult, MessagePage } from '../api/types'

import type {
  RuntimeAdapter,
  RuntimeMessageCommandRequest,
  RuntimeMessagePageRequest,
} from './types'

type QueuedOutcome<T> =
  | { kind: 'resolve'; value: T }
  | { kind: 'reject'; error: Error }

const defaultMessagePage: MessagePage = {
  items: [],
  nextCursor: null,
}

const defaultMessageCommandResult: MessageCommandResult = {
  detail: null,
  events: [],
}

export interface FakeRuntimeAdapter extends RuntimeAdapter {
  readonly messageCommandCalls: RuntimeMessageCommandRequest[]
  readonly messagePageCalls: RuntimeMessagePageRequest[]
  queueMessageCommandResult(result: MessageCommandResult): void
  queueMessageCommandError(error: Error): void
  queueMessagePage(page: MessagePage): void
  queueMessagePageError(error: Error): void
  reset(): void
}

/** Fake runtime adapter for renderer tests. Does not start or contact a backend. */
export function createFakeRuntimeAdapter(input?: {
  defaultMessageCommandResult?: MessageCommandResult
  defaultMessagePage?: MessagePage
}): FakeRuntimeAdapter {
  const messageCommandCalls: RuntimeMessageCommandRequest[] = []
  const messagePageCalls: RuntimeMessagePageRequest[] = []
  const queuedMessageCommandOutcomes: QueuedOutcome<MessageCommandResult>[] = []
  const queuedMessagePageOutcomes: QueuedOutcome<MessagePage>[] = []
  const commandFallback =
    input?.defaultMessageCommandResult ?? defaultMessageCommandResult
  const pageFallback = input?.defaultMessagePage ?? defaultMessagePage

  return {
    messageCommandCalls,
    messagePageCalls,
    queueMessageCommandResult(result) {
      queuedMessageCommandOutcomes.push({ kind: 'resolve', value: result })
    },
    queueMessageCommandError(error) {
      queuedMessageCommandOutcomes.push({ kind: 'reject', error })
    },
    queueMessagePage(page) {
      queuedMessagePageOutcomes.push({ kind: 'resolve', value: page })
    },
    queueMessagePageError(error) {
      queuedMessagePageOutcomes.push({ kind: 'reject', error })
    },
    reset() {
      messageCommandCalls.length = 0
      messagePageCalls.length = 0
      queuedMessageCommandOutcomes.length = 0
      queuedMessagePageOutcomes.length = 0
    },
    async fetchMessagePage(request) {
      messagePageCalls.push({ ...request })
      const next = queuedMessagePageOutcomes.shift()
      if (!next) {
        return pageFallback
      }
      if (next.kind === 'reject') {
        throw next.error
      }
      return next.value
    },
    async runMessageCommand(request) {
      messageCommandCalls.push({ ...request })
      const next = queuedMessageCommandOutcomes.shift()
      if (!next) {
        return commandFallback
      }
      if (next.kind === 'reject') {
        throw next.error
      }
      return next.value
    },
  }
}
