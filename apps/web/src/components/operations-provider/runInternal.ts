import type { QueryClient } from '@tanstack/react-query'

import type { KnownMailboxRole, Mailbox, MessageCommand } from '../../api/types'
import { runtimeMutations } from '../../runtime/mutations'
import { runtimeViews } from '../../runtime/views'
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
  OperationKind,
  OperationTarget,
} from '../../operations'
import { queryKeys } from '../../queryKeys'

export interface RunOutcome {
  applied: AppliedOperation | null
  ok: boolean
}

function neutralState(): MutableState {
  return { keywords: [], mailboxIds: [] }
}

function requiredMailboxByRole(
  mailboxes: Mailbox[] | undefined,
  sourceId: string,
  role: KnownMailboxRole,
): string {
  const mailbox = mailboxes?.find((candidate) => candidate.role === role)
  if (!mailbox) {
    throw new Error(`Missing mailbox with role ${role} for source ${sourceId}`)
  }
  return mailbox.id
}

async function resolveRoleMailboxIds(
  queryClient: QueryClient,
  operation: MailOperation,
): Promise<Map<string, string>> {
  const roleMailboxIds = new Map<string, string>()
  if (operation.kind !== 'mailbox-role' || !operation.mailboxRole) {
    return roleMailboxIds
  }
  for (const target of operation.targets) {
    if (roleMailboxIds.has(target.sourceId)) {
      continue
    }
    const mailboxes =
      queryClient.getQueryData<Mailbox[]>(
        queryKeys.mailboxes(target.sourceId),
      ) ??
      (await queryClient.ensureQueryData({
        queryFn: () => runtimeViews.mail.mailboxes(target.sourceId),
        queryKey: queryKeys.mailboxes(target.sourceId),
      }))
    roleMailboxIds.set(
      target.sourceId,
      requiredMailboxByRole(mailboxes, target.sourceId, operation.mailboxRole),
    )
  }
  return roleMailboxIds
}

function projectAfter(
  operation: MailOperation,
  target: OperationTarget,
  before: MutableState,
  roleMailboxIds: Map<string, string>,
): MutableState {
  if (operation.kind === 'destroy') {
    return before
  }
  if (operation.kind === 'mailbox-role') {
    const mailboxId = roleMailboxIds.get(target.sourceId)
    if (!mailboxId) {
      throw new Error(
        `Missing resolved mailbox role for source ${target.sourceId}`,
      )
    }
    return { ...before, mailboxIds: [mailboxId] }
  }
  if (!operation.project) {
    throw new Error(`Operation ${operation.kind} is missing its projector`)
  }
  return operation.project(target, before)
}

function appliedOperationKind(kind: OperationKind): OperationKind {
  return kind === 'mailbox-role' ? 'mailboxes' : kind
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
  let roleMailboxIds: Map<string, string>
  try {
    roleMailboxIds = await resolveRoleMailboxIds(queryClient, operation)
  } catch (error) {
    setErrorMessage(error instanceof Error ? error.message : 'Operation failed')
    return { applied: null, ok: false }
  }
  const prepared = operation.targets.map((rawTarget) => {
    const conversationId =
      rawTarget.conversationId ??
      findConversationIdForMessage(queryClient, rawTarget) ??
      undefined
    const target: OperationTarget = { ...rawTarget, conversationId }
    const captured = captureMutableState(queryClient, target)
    const before = captured ?? neutralState()
    return {
      after: projectAfter(operation, target, before, roleMailboxIds),
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
      const result = applyMailboxPatch(
        queryClient,
        selection,
        after.mailboxIds,
        {
          destroy,
        },
      )
      snapshots.push(...result.snapshots)
    }
    if (
      !destroy &&
      commands.some((command) => command.kind === 'setKeywords')
    ) {
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
      if (operation.kind === 'mailbox-role') {
        if (!operation.mailboxRole) {
          throw new Error('Mailbox role operation is missing its role')
        }
        const result = await runtimeMutations.messages.moveToMailboxRole({
          messageId: target.messageId,
          role: operation.mailboxRole,
          sourceId: target.sourceId,
        })
        recordLocalMutationEvents(result.events)
        if (result.detail && target.conversationId) {
          mergeMessageDetail(queryClient, result.detail, target.conversationId)
        }
      } else {
        const commands = destroy
          ? [{ kind: 'destroy' } as MessageCommand]
          : diffMutableState(before, after)
        for (const command of commands) {
          const result = await runtimeMutations.messages.command({
            command,
            messageId: target.messageId,
            sourceId: target.sourceId,
          })
          recordLocalMutationEvents(result.events)
          if (!destroy && result.detail && target.conversationId) {
            mergeMessageDetail(
              queryClient,
              result.detail,
              target.conversationId,
            )
          }
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
        kind: appliedOperationKind(operation.kind),
        label: operation.label,
        undoLabel: operation.undoLabel,
      }
    : null
  return { applied, ok: true }
}
