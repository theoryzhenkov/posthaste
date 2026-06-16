import type {
  Mailbox,
  MessageCommandResult,
  MessagePage,
  ReadRequest,
  ReadResponse,
  SmartMailboxSummary,
} from '../api/types'

import type {
  RuntimeAdapter,
  RuntimeMessageCommandRequest,
  RuntimeMessagePageRequest,
} from './types'

type QueuedOutcome<T> =
  | { kind: 'resolve'; value: T }
  | { kind: 'reject'; error: Error }

const defaultReadResponse: ReadResponse = { results: {} }
const defaultMailboxes: Mailbox[] = []
const defaultSmartMailboxes: SmartMailboxSummary[] = []

const defaultMessagePage: MessagePage = {
  items: [],
  nextCursor: null,
}

const defaultMessageCommandResult: MessageCommandResult = {
  detail: null,
  events: [],
}

function queueResolve<T>(queue: QueuedOutcome<T>[], value: T): void {
  queue.push({ kind: 'resolve', value })
}

function queueReject<T>(queue: QueuedOutcome<T>[], error: Error): void {
  queue.push({ kind: 'reject', error })
}

function resolveQueued<T>(queue: QueuedOutcome<T>[], fallback: T): Promise<T> {
  const next = queue.shift()
  if (!next) {
    return Promise.resolve(fallback)
  }
  if (next.kind === 'reject') {
    return Promise.reject(next.error)
  }
  return Promise.resolve(next.value)
}

export interface FakeRuntimeAdapter extends RuntimeAdapter {
  readonly mailboxCalls: string[]
  readonly messageCommandCalls: RuntimeMessageCommandRequest[]
  readonly messagePageCalls: RuntimeMessagePageRequest[]
  readonly readCalls: ReadRequest[]
  readonly smartMailboxCalls: number
  queueMailboxes(mailboxes: Mailbox[]): void
  queueMailboxesError(error: Error): void
  queueMessageCommandResult(result: MessageCommandResult): void
  queueMessageCommandError(error: Error): void
  queueMessagePage(page: MessagePage): void
  queueMessagePageError(error: Error): void
  queueReadResponse(response: ReadResponse): void
  queueReadError(error: Error): void
  queueSmartMailboxes(mailboxes: SmartMailboxSummary[]): void
  queueSmartMailboxesError(error: Error): void
  reset(): void
}

/** Fake runtime adapter for renderer tests. Does not start or contact a backend. */
export function createFakeRuntimeAdapter(input?: {
  defaultMailboxes?: Mailbox[]
  defaultMessageCommandResult?: MessageCommandResult
  defaultMessagePage?: MessagePage
  defaultReadResponse?: ReadResponse
  defaultSmartMailboxes?: SmartMailboxSummary[]
}): FakeRuntimeAdapter {
  const mailboxCalls: string[] = []
  const messageCommandCalls: RuntimeMessageCommandRequest[] = []
  const messagePageCalls: RuntimeMessagePageRequest[] = []
  const readCalls: ReadRequest[] = []
  const queuedMailboxes: QueuedOutcome<Mailbox[]>[] = []
  const queuedMessageCommands: QueuedOutcome<MessageCommandResult>[] = []
  const queuedMessagePages: QueuedOutcome<MessagePage>[] = []
  const queuedReads: QueuedOutcome<ReadResponse>[] = []
  const queuedSmartMailboxes: QueuedOutcome<SmartMailboxSummary[]>[] = []
  let smartMailboxCalls = 0

  return {
    mailboxCalls,
    messageCommandCalls,
    messagePageCalls,
    readCalls,
    get smartMailboxCalls() {
      return smartMailboxCalls
    },
    queueMailboxes(mailboxes) {
      queueResolve(queuedMailboxes, mailboxes)
    },
    queueMailboxesError(error) {
      queueReject(queuedMailboxes, error)
    },
    queueMessageCommandResult(result) {
      queueResolve(queuedMessageCommands, result)
    },
    queueMessageCommandError(error) {
      queueReject(queuedMessageCommands, error)
    },
    queueMessagePage(page) {
      queueResolve(queuedMessagePages, page)
    },
    queueMessagePageError(error) {
      queueReject(queuedMessagePages, error)
    },
    queueReadResponse(response) {
      queueResolve(queuedReads, response)
    },
    queueReadError(error) {
      queueReject(queuedReads, error)
    },
    queueSmartMailboxes(mailboxes) {
      queueResolve(queuedSmartMailboxes, mailboxes)
    },
    queueSmartMailboxesError(error) {
      queueReject(queuedSmartMailboxes, error)
    },
    reset() {
      mailboxCalls.length = 0
      messageCommandCalls.length = 0
      messagePageCalls.length = 0
      readCalls.length = 0
      queuedMailboxes.length = 0
      queuedMessageCommands.length = 0
      queuedMessagePages.length = 0
      queuedReads.length = 0
      queuedSmartMailboxes.length = 0
      smartMailboxCalls = 0
    },
    fetchMailboxes(accountId) {
      mailboxCalls.push(accountId)
      return resolveQueued(queuedMailboxes, input?.defaultMailboxes ?? defaultMailboxes)
    },
    fetchMessagePage(request) {
      messagePageCalls.push({ ...request })
      return resolveQueued(queuedMessagePages, input?.defaultMessagePage ?? defaultMessagePage)
    },
    fetchSmartMailboxes() {
      smartMailboxCalls += 1
      return resolveQueued(
        queuedSmartMailboxes,
        input?.defaultSmartMailboxes ?? defaultSmartMailboxes,
      )
    },
    read(request) {
      readCalls.push(request)
      return resolveQueued(queuedReads, input?.defaultReadResponse ?? defaultReadResponse)
    },
    runMessageCommand(request) {
      messageCommandCalls.push({ ...request })
      return resolveQueued(
        queuedMessageCommands,
        input?.defaultMessageCommandResult ?? defaultMessageCommandResult,
      )
    },
  }
}
