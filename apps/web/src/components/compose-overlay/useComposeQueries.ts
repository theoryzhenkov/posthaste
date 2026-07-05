import { useCallback, useMemo } from 'react'
import { useQuery, useQueryClient } from '@tanstack/react-query'

import type { MessageDetail, Recipient } from '@/api/types'
import { buildRecipientSuggestionOptions } from '@/composeAddressSuggestions'
import type { ComposeIntent } from '@/composeIntent'
import { mailKeys } from '@/mailState'
import { queryKeys } from '@/queryKeys'
import { runtimeViews } from '@/runtime/views'

import {
  accountFromOptions,
  formatRecipient,
  formatRecipients,
  replyContextFromCachedMessage,
  wildcardMatchesEmail,
} from '../composeFormHelpers'

export function useComposeQueries({ intent }: { intent: ComposeIntent }) {
  const queryClient = useQueryClient()
  const identityQuery = useQuery({
    queryKey: queryKeys.identity(intent.sourceId),
    queryFn: () => runtimeViews.compose.identity(intent.sourceId),
  })
  const accountsQuery = useQuery({
    queryKey: queryKeys.accounts,
    queryFn: runtimeViews.accounts.list,
  })
  const senderAddressQuery = useQuery({
    queryKey: queryKeys.senderAddresses,
    queryFn: runtimeViews.compose.senderAddresses,
  })
  const recipientSuggestionQuery = useQuery({
    queryKey: queryKeys.composeRecipientSuggestions,
    queryFn: () =>
      runtimeViews.compose.conversationPage({
        limit: 75,
        sort: 'date',
        sortDir: 'desc',
      }),
  })
  const isMessageBasedCompose = intent.kind !== 'new'
  const requiresMessageContext =
    intent.kind === 'reply' ||
    intent.kind === 'replyAll' ||
    intent.kind === 'forward'
  const isDraftEdit = intent.kind === 'draft'
  const draftSeedQuery = useQuery({
    queryKey: isDraftEdit
      ? ['draft-seed', intent.sourceId, intent.messageId]
      : ['draft-seed', null],
    queryFn: () =>
      runtimeViews.compose.draftContent({
        sourceId: intent.sourceId,
        messageId: isDraftEdit ? intent.messageId : '',
      }),
    enabled: isDraftEdit,
  })
  const draftSeed = useMemo(
    () =>
      draftSeedQuery.data
        ? {
            from: draftSeedQuery.data.from
              ? formatRecipient(draftSeedQuery.data.from)
              : '',
            to: formatRecipients(draftSeedQuery.data.to),
            cc: formatRecipients(draftSeedQuery.data.cc),
            bcc: formatRecipients(draftSeedQuery.data.bcc),
            subject: draftSeedQuery.data.subject,
            body: draftSeedQuery.data.body,
          }
        : undefined,
    [draftSeedQuery.data],
  )
  // FIX2 — seed the reply composer's quote from the detail-pane cache the user
  // just had open, so the quoted body + reply recipient + subject appear
  // INSTANTLY instead of blocking on the fresh `replyContext` Email/get. Only a
  // PLAIN reply is cache-seedable (the cache lacks the References header + Cc
  // list a reply-all/send needs); the authoritative fetch still runs and
  // supplies those before send (the composer gates that on `isPlaceholderData`).
  const plainReplyMessageId = intent.kind === 'reply' ? intent.messageId : null
  const replyContextPlaceholder = useMemo(() => {
    if (!plainReplyMessageId) {
      return undefined
    }
    const cached = queryClient.getQueryData<MessageDetail>(
      mailKeys.message(intent.sourceId, plainReplyMessageId),
    )
    return cached ? replyContextFromCachedMessage(cached) : undefined
  }, [intent.sourceId, plainReplyMessageId, queryClient])
  const replyContextQuery = useQuery({
    queryKey: requiresMessageContext
      ? ['reply-context', intent.sourceId, intent.messageId]
      : ['reply-context', null],
    queryFn: () =>
      runtimeViews.compose.replyContext({
        sourceId: intent.sourceId,
        messageId: isMessageBasedCompose ? intent.messageId : '',
      }),
    enabled: requiresMessageContext,
    // A cache-built placeholder unblocks the quote immediately; react-query
    // still fetches the authoritative context in the background and replaces it.
    ...(replyContextPlaceholder
      ? { placeholderData: replyContextPlaceholder }
      : {}),
  })
  const composeKey = isMessageBasedCompose
    ? `${intent.kind}:${intent.sourceId}:${intent.messageId}`
    : `new:${intent.sourceId}`

  const fromIdentity = useMemo(
    () =>
      identityQuery.data
        ? {
            name: identityQuery.data.name || null,
            email: identityQuery.data.email,
          }
        : null,
    [identityQuery.data],
  )
  const fromOptions = useMemo(
    () =>
      accountFromOptions(
        accountsQuery.data ?? [],
        fromIdentity,
        intent.sourceId,
        senderAddressQuery.data ?? [],
      ),
    [
      accountsQuery.data,
      fromIdentity,
      intent.sourceId,
      senderAddressQuery.data,
    ],
  )
  const recipientSuggestions = useMemo(
    () =>
      buildRecipientSuggestionOptions(accountsQuery.data ?? [], [
        recipientSuggestionQuery.data,
      ]),
    [accountsQuery.data, recipientSuggestionQuery.data],
  )

  const resolveSubmissionSourceId = useCallback(
    (from: Recipient | null): string => {
      const email = from?.email.trim().toLowerCase()
      if (!email) {
        return intent.sourceId
      }
      const exact = fromOptions.find(
        (option) => option.email.toLowerCase() === email,
      )
      if (exact) {
        return exact.sourceId
      }
      const accounts = accountsQuery.data ?? []
      const currentAccount = accounts.find(
        (account) => account.id === intent.sourceId,
      )
      if (
        currentAccount?.emailPatterns.some((pattern) =>
          wildcardMatchesEmail(pattern, email),
        )
      ) {
        return currentAccount.id
      }
      return (
        accounts.find((account) =>
          account.emailPatterns.some((pattern) =>
            wildcardMatchesEmail(pattern, email),
          ),
        )?.id ?? intent.sourceId
      )
    },
    [accountsQuery.data, fromOptions, intent.sourceId],
  )

  return {
    accountsQuery,
    composeKey,
    draftSeed,
    // Stable identity of the resumed draft, once its content loads. Autosave
    // keys by this so an edit updates the draft in place across provider id
    // rotation; `null` for legacy drafts without the header (keyed by id).
    draftSeedDraftId: draftSeedQuery.data?.draftId ?? null,
    draftSeedQuery,
    fromOptions,
    identityQuery,
    isDraftEdit,
    isMessageBasedCompose,
    recipientSuggestions,
    replyContextQuery,
    requiresMessageContext,
    resolveSubmissionSourceId,
  }
}
