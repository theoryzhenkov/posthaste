import { useCallback, useMemo } from 'react'
import { useQuery } from '@tanstack/react-query'

import type { Recipient } from '@/api/types'
import { buildRecipientSuggestionOptions } from '@/composeAddressSuggestions'
import type { ComposeIntent } from '@/composeIntent'
import { queryKeys } from '@/queryKeys'
import { runtimeViews } from '@/runtime/views'

import {
  accountFromOptions,
  formatRecipients,
  wildcardMatchesEmail,
} from '../composeFormHelpers'

export function useComposeQueries({ intent }: { intent: ComposeIntent }) {
  const identityQuery = useQuery({
    queryKey: ['identity', intent.sourceId],
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
    intent.kind === 'reply' || intent.kind === 'forward'
  const isDraftEdit = intent.kind === 'draft'
  // Resume-editing seeds the form from the existing draft message. Note: the
  // message detail exposes `to` but not cc/bcc (those live only in the stored
  // MIME), so cc/bcc are not restored when editing a synced draft.
  const draftSeedQuery = useQuery({
    queryKey: isDraftEdit
      ? ['draft-seed', intent.sourceId, intent.messageId]
      : ['draft-seed', null],
    queryFn: () =>
      runtimeViews.mail.message(
        isDraftEdit ? intent.messageId : '',
        intent.sourceId,
      ),
    enabled: isDraftEdit,
  })
  const draftSeed = useMemo(
    () =>
      draftSeedQuery.data
        ? {
            to: formatRecipients(draftSeedQuery.data.to),
            subject: draftSeedQuery.data.subject ?? '',
            body: draftSeedQuery.data.bodyText ?? '',
          }
        : undefined,
    [draftSeedQuery.data],
  )
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
