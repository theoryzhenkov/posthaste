import type { QueryClient } from '@tanstack/react-query'

import { performMessageCommand } from '../../api/client'
import type { MessageCommand } from '../../api/types'
import {
  invalidateMessageMutationReadModels,
  invalidateMessageScopeReadModels,
} from '../../domainCache'
import {
  applyKeywordPatch,
  applyMailboxPatch,
  captureMutableState,
  deriveKeywordState,
  diffMutableState,
  findConversationIdForMessage,
  mergeMessageDetail,
  recordLocalMutationEvents,
  restoreSnapshots,
  type MailSelection,
  type MutableState,
  type QuerySnapshot,
} from '../../mailState'
import type {
  AppliedOperation,
  MailOperation,
  OperationEntry,
  OperationTarget,
} from '../../operations'

export interface RunOutcome {
  applied: AppliedOperation | null
  ok: boolean
}

function neutralState(): MutableState {
  return { keywords: [], mailboxIds: [] }
}

function selectionFor(target: OperationTarget): MailSelection {
  return {
    conversationId: target.conversationId ?? '',
    messageId: target.messageId,
    sourceId: target.sourceId,
  }
}

export async function runOperationInternal(input: {
  operation: MailOperation
  queryClient: QueryClient
  setErrorMessage: (message: string | null) => void
  setPending: (delta: number) => void
}): Promise<RunOutcome> {
  const { operation, queryClient, setErrorMessage, setPending } = input
  const destroy = operation.kind === 'destroy'
  const prepared = operation.targets.map((rawTarget) => {
    const conversationId =
      rawTarget.conversationId ??
      findConversationIdForMessage(queryClient, rawTarget) ??
      undefined
    const target: OperationTarget = { ...rawTarget, conversationId }
    const captured = captureMutableState(queryClient, target)
    const before = captured ?? neutralState()
    return {
      after: destroy ? before : operation.project(target, before),
      before,
      captured: captured !== null,
      target,
    }
  })

  const invertible = !destroy && prepared.every((entry) => entry.captured)
  const snapshots: QuerySnapshot[] = []
  const recordedEntries: OperationEntry[] = []

  for (const { target, before, after, captured } of prepared) {
    if (captured) {
      recordedEntries.push({ after, before, target })
    }
    if (!captured) {
      continue
    }
    const selection = selectionFor(target)
    const commands = destroy
      ? [{ kind: 'destroy' } as MessageCommand]
      : diffMutableState(before, after)
    if (
      destroy ||
      commands.some((command) => command.kind === 'replaceMailboxes')
    ) {
      const result = applyMailboxPatch(queryClient, selection, after.mailboxIds, {
        destroy,
      })
      snapshots.push(...result.snapshots)
    }
    if (!destroy && commands.some((command) => command.kind === 'setKeywords')) {
      const result = applyKeywordPatch(queryClient, selection, {
        next: deriveKeywordState(after.keywords),
        previous: deriveKeywordState(before.keywords),
      })
      snapshots.push(...result.snapshots)
    }
  }

  setPending(1)
  try {
    for (const { target, before, after } of prepared) {
      const commands = destroy
        ? [{ kind: 'destroy' } as MessageCommand]
        : diffMutableState(before, after)
      for (const command of commands) {
        const result = await performMessageCommand(
          target.messageId,
          command,
          target.sourceId,
        )
        recordLocalMutationEvents(result.events)
        if (!destroy && result.detail && target.conversationId) {
          mergeMessageDetail(queryClient, result.detail, target.conversationId)
        }
      }
      void invalidateMessageMutationReadModels(queryClient, target)
      void invalidateMessageScopeReadModels(
        queryClient,
        target,
        target.conversationId ?? null,
      )
    }
  } catch (error) {
    if (snapshots.length) {
      restoreSnapshots(queryClient, snapshots)
    }
    for (const { target } of prepared) {
      void invalidateMessageMutationReadModels(queryClient, target)
    }
    setErrorMessage(error instanceof Error ? error.message : 'Operation failed')
    return { applied: null, ok: false }
  } finally {
    setPending(-1)
  }

  const applied: AppliedOperation | null = invertible
    ? {
        entries: recordedEntries,
        invertible: true,
        kind: operation.kind,
        label: operation.label,
        undoLabel: operation.undoLabel,
      }
    : null
  return { applied, ok: true }
}
