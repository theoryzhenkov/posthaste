/**
 * React hook that provides optimistic email actions (toggle read/flag, archive, trash, delete).
 *
 * Mutations apply optimistic keyword patches to the cache, record local mutation
 * events for echo suppression, and fall back to invalidation when the cache is incomplete.
 *
 * @spec docs/L1-ui#data-fetching
 */
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { useState } from 'react'
import { toast } from 'sonner'
import { fetchSidebar, performMessageCommand } from '../api/client'
import type {
  KnownMailboxRole,
  MessageCommand,
  MessageDetail,
  MessageSummary,
  SidebarResponse,
  SourceMessageRef,
} from '../api/types'
import {
  applyKeywordPatch,
  deriveKeywordState,
  findConversationIdForMessage,
  mailKeys,
  mergeMessageDetail,
  recordLocalMutationEvents,
  restoreSnapshots,
  type KeywordPatch,
  type MailSelection,
  type QuerySnapshot,
} from '../mailState'
import { queryKeys } from '../queryKeys'

/** Message reference augmented with optional keyword fields for optimistic patching. */
type ReadToggleTarget = MailSelection &
  Partial<Pick<MessageSummary, 'isFlagged' | 'isRead' | 'keywords'>>
/** Message reference augmented with optional keyword fields for optimistic patching. */
type FlagToggleTarget = MailSelection &
  Partial<Pick<MessageSummary, 'isFlagged' | 'isRead' | 'keywords'>>

type MutationInput =
  | {
      command: MessageCommand
      conversationId?: string
      optimisticKeywordPatch?: KeywordPatch
      target: SourceMessageRef
    }
  | {
      conversationId?: string
      mailboxRole: KnownMailboxRole
      target: SourceMessageRef
    }

type MutationContext = {
  conversationId: string | null
  incomplete: boolean
  isKeywordMutation: boolean
  snapshots: QuerySnapshot[]
  target: SourceMessageRef
}

/** Return type of {@link useEmailActions}. */
export type EmailActions = ReturnType<typeof useEmailActions>

function requiredMailboxByRole(
  sidebar: SidebarResponse | undefined,
  sourceId: string,
  role: KnownMailboxRole,
) {
  const source = sidebar?.sources.find((candidate) => candidate.id === sourceId)
  const mailbox = source?.mailboxes.find((candidate) => candidate.role === role)
  if (!mailbox) {
    throw new Error(`Missing mailbox with role ${role} for source ${sourceId}`)
  }
  return mailbox
}

function toSourceMessageRef(
  message: SourceMessageRef | MessageSummary | MailSelection,
): SourceMessageRef {
  if ('messageId' in message) {
    return { sourceId: message.sourceId, messageId: message.messageId }
  }
  return { sourceId: message.sourceId, messageId: message.id }
}

function toMailSelection(
  queryClient: ReturnType<typeof useQueryClient>,
  message: MailSelection | MessageSummary | SourceMessageRef,
): MailSelection | null {
  if ('conversationId' in message) {
    if ('messageId' in message) {
      return message
    }
    return {
      conversationId: message.conversationId,
      messageId: message.id,
      sourceId: message.sourceId,
    }
  }

  const conversationId = findConversationIdForMessage(queryClient, message)
  if (!conversationId) {
    return null
  }

  return {
    conversationId,
    messageId: message.messageId,
    sourceId: message.sourceId,
  }
}

function synthesizeKeywords(
  message: Partial<Pick<MessageSummary, 'isFlagged' | 'isRead' | 'keywords'>>,
) {
  if (message.keywords) {
    return message.keywords
  }

  const keywords: string[] = []
  if (message.isRead) {
    keywords.push('$seen')
  }
  if (message.isFlagged) {
    keywords.push('$flagged')
  }
  return keywords
}

function resolveKeywordState(
  queryClient: ReturnType<typeof useQueryClient>,
  message: ReadToggleTarget | FlagToggleTarget | MessageSummary,
) {
  if ('keywords' in message && Array.isArray(message.keywords)) {
    return deriveKeywordState(message.keywords)
  }

  const target = toSourceMessageRef(message)
  const cachedMessage = queryClient.getQueryData<MessageDetail>(
    mailKeys.message(target.sourceId, target.messageId),
  )
  if (cachedMessage) {
    return deriveKeywordState(cachedMessage.keywords)
  }

  return deriveKeywordState(synthesizeKeywords(message))
}

function applyKeywordToggle(
  current: string[],
  keyword: '$flagged' | '$seen',
  enabled: boolean,
) {
  if (enabled) {
    return current.includes(keyword) ? current : [...current, keyword]
  }
  return current.filter((candidate) => candidate !== keyword)
}

function normalizeUserTag(tag: string): string | null {
  const normalized = tag.trim().replace(/\s+/g, ' ')
  if (!normalized || normalized.startsWith('$') || normalized.includes('/')) {
    return null
  }
  return normalized
}

function userTagsFromKeywords(keywords: string[]): string[] {
  return keywords.filter((keyword) => !keyword.startsWith('$'))
}

function uniqueUserTags(tags: string[]): string[] {
  const seen = new Set<string>()
  const unique: string[] = []
  for (const tag of tags) {
    const normalized = normalizeUserTag(tag)
    if (!normalized) {
      continue
    }
    const key = normalized.toLowerCase()
    if (seen.has(key)) {
      continue
    }
    seen.add(key)
    unique.push(normalized)
  }
  return unique
}

function invalidateMessageScope(
  queryClient: ReturnType<typeof useQueryClient>,
  target: SourceMessageRef,
  conversationId: string | null,
) {
  queryClient.invalidateQueries({
    queryKey: mailKeys.message(target.sourceId, target.messageId),
  })
  if (conversationId) {
    queryClient.invalidateQueries({
      queryKey: mailKeys.conversation(conversationId),
    })
    queryClient.invalidateQueries({
      queryKey: mailKeys.conversationSummary(conversationId),
    })
  }
  queryClient.invalidateQueries({ queryKey: ['conversations'] })
}

/**
 * Provides optimistic email action methods: `toggleRead`, `toggleFlag`,
 * `archive`, `trash`, and `deletePermanently`.
 *
 * Uses optimistic cache patches with rollback on error.
 *
 * @spec docs/L1-ui#data-fetching
 */
export function useEmailActions() {
  const queryClient = useQueryClient()
  const [errorMessage, setErrorMessage] = useState<string | null>(null)

  const mutation = useMutation({
    mutationFn: async (input: MutationInput) => {
      const command =
        'mailboxRole' in input
          ? await replaceMailboxCommandByRole(
              queryClient,
              input.target,
              input.mailboxRole,
            )
          : input.command
      return performMessageCommand(
        input.target.messageId,
        command,
        input.target.sourceId,
      )
    },
    onMutate: (input): MutationContext => {
      setErrorMessage(null)

      const conversationId =
        input.conversationId ??
        findConversationIdForMessage(queryClient, input.target)
      const snapshots: QuerySnapshot[] = []
      let incomplete = false

      if (
        conversationId &&
        'optimisticKeywordPatch' in input &&
        input.optimisticKeywordPatch
      ) {
        const optimisticResult = applyKeywordPatch(
          queryClient,
          { ...input.target, conversationId },
          input.optimisticKeywordPatch,
        )
        snapshots.push(...optimisticResult.snapshots)
        incomplete = optimisticResult.incomplete
      }

      return {
        conversationId,
        incomplete,
        isKeywordMutation:
          'optimisticKeywordPatch' in input && !!input.optimisticKeywordPatch,
        snapshots,
        target: input.target,
      }
    },
    onSuccess: (data, input, context) => {
      recordLocalMutationEvents(data.events)

      const conversationId =
        context?.conversationId ?? data.detail?.conversationId ?? null

      if (context?.isKeywordMutation && data.detail && conversationId) {
        const merged = mergeMessageDetail(
          queryClient,
          data.detail,
          conversationId,
        )
        if (!merged) {
          context.incomplete = true
        }
        queryClient.invalidateQueries({ queryKey: queryKeys.sidebar })
        queryClient.invalidateQueries({ queryKey: queryKeys.smartMailboxes })
        return
      }

      queryClient.invalidateQueries({ queryKey: queryKeys.sidebar })
      queryClient.invalidateQueries({ queryKey: queryKeys.smartMailboxes })
      invalidateMessageScope(queryClient, input.target, conversationId)
    },
    onError: (error, _input, context) => {
      if (context?.snapshots.length) {
        restoreSnapshots(queryClient, context.snapshots)
      }
      setErrorMessage(error.message)
    },
    onSettled: (_data, _error, _input, context) => {
      if (context?.isKeywordMutation && context.incomplete) {
        invalidateMessageScope(
          queryClient,
          context.target,
          context.conversationId,
        )
      }
    },
  })

  return {
    toggleRead: (message: ReadToggleTarget | MessageSummary) => {
      const selection = toMailSelection(queryClient, message)
      const previous = resolveKeywordState(queryClient, message)
      const nextKeywords = applyKeywordToggle(
        previous.keywords,
        '$seen',
        !previous.isRead,
      )
      mutation.mutate({
        command: previous.isRead
          ? { kind: 'setKeywords', add: [], remove: ['$seen'] }
          : { kind: 'setKeywords', add: ['$seen'], remove: [] },
        conversationId: selection?.conversationId,
        optimisticKeywordPatch: {
          next: deriveKeywordState(nextKeywords),
          previous,
        },
        target: toSourceMessageRef(message),
      })
    },
    markRead: (message: ReadToggleTarget | MessageSummary) => {
      const selection = toMailSelection(queryClient, message)
      const previous = resolveKeywordState(queryClient, message)
      if (previous.isRead) {
        return
      }
      const nextKeywords = applyKeywordToggle(previous.keywords, '$seen', true)
      mutation.mutate({
        command: { kind: 'setKeywords', add: ['$seen'], remove: [] },
        conversationId: selection?.conversationId,
        optimisticKeywordPatch: {
          next: deriveKeywordState(nextKeywords),
          previous,
        },
        target: toSourceMessageRef(message),
      })
    },
    toggleFlag: (message: FlagToggleTarget | MessageSummary) => {
      const selection = toMailSelection(queryClient, message)
      const previous = resolveKeywordState(queryClient, message)
      const nextKeywords = applyKeywordToggle(
        previous.keywords,
        '$flagged',
        !previous.isFlagged,
      )
      mutation.mutate({
        command: previous.isFlagged
          ? { kind: 'setKeywords', add: [], remove: ['$flagged'] }
          : { kind: 'setKeywords', add: ['$flagged'], remove: [] },
        conversationId: selection?.conversationId,
        optimisticKeywordPatch: {
          next: deriveKeywordState(nextKeywords),
          previous,
        },
        target: toSourceMessageRef(message),
      })
    },
    setUserTags: (
      message: (ReadToggleTarget | MessageSummary) & {
        keywords?: string[]
      },
      tags: string[],
    ) => {
      const selection = toMailSelection(queryClient, message)
      const previous = resolveKeywordState(queryClient, message)
      const previousUserTags = userTagsFromKeywords(previous.keywords)
      const nextUserTags = uniqueUserTags(tags)
      const systemKeywords = previous.keywords.filter((keyword) =>
        keyword.startsWith('$'),
      )
      const add = nextUserTags.filter(
        (tag) => !previousUserTags.some((current) => current === tag),
      )
      const remove = previousUserTags.filter(
        (tag) => !nextUserTags.some((next) => next === tag),
      )
      if (add.length === 0 && remove.length === 0) {
        return
      }
      const nextKeywords = [...systemKeywords, ...nextUserTags]
      mutation.mutate({
        command: { kind: 'setKeywords', add, remove },
        conversationId: selection?.conversationId,
        optimisticKeywordPatch: {
          next: deriveKeywordState(nextKeywords),
          previous,
        },
        target: toSourceMessageRef(message),
      })
    },
    archive: (target: SourceMessageRef) => {
      mutation.mutate(
        {
          conversationId:
            findConversationIdForMessage(queryClient, target) ?? undefined,
          mailboxRole: 'archive',
          target,
        },
        {
          onSuccess: () => {
            toast('Message archived', {
              duration: 5000,
              action: {
                label: 'Undo',
                onClick: () =>
                  mutation.mutate({
                    conversationId:
                      findConversationIdForMessage(queryClient, target) ??
                      undefined,
                    mailboxRole: 'inbox',
                    target,
                  }),
              },
            })
          },
        },
      )
    },
    trash: (target: SourceMessageRef) => {
      mutation.mutate(
        {
          conversationId:
            findConversationIdForMessage(queryClient, target) ?? undefined,
          mailboxRole: 'trash',
          target,
        },
        {
          onSuccess: () => {
            toast('Message trashed', {
              duration: 5000,
              action: {
                label: 'Undo',
                onClick: () =>
                  mutation.mutate({
                    conversationId:
                      findConversationIdForMessage(queryClient, target) ??
                      undefined,
                    mailboxRole: 'inbox',
                    target,
                  }),
              },
            })
          },
        },
      )
    },
    deletePermanently: (target: SourceMessageRef) =>
      mutation.mutate({
        command: { kind: 'destroy' },
        conversationId:
          findConversationIdForMessage(queryClient, target) ?? undefined,
        target,
      }),
    clearError: () => setErrorMessage(null),
    errorMessage,
    isPending: mutation.isPending,
  }
}

async function replaceMailboxCommandByRole(
  queryClient: ReturnType<typeof useQueryClient>,
  target: SourceMessageRef,
  role: KnownMailboxRole,
): Promise<MessageCommand> {
  const sidebar =
    queryClient.getQueryData<SidebarResponse>(queryKeys.sidebar) ??
    (await queryClient.ensureQueryData({
      queryFn: fetchSidebar,
      queryKey: queryKeys.sidebar,
    }))
  const mailbox = requiredMailboxByRole(sidebar, target.sourceId, role)
  return {
    kind: 'replaceMailboxes',
    mailboxIds: [mailbox.id],
  }
}
