import { useCallback, useMemo } from 'react'
import { useQuery } from '@tanstack/react-query'

import type { Recipient } from '@/api/types'
import { buildRecipientSuggestionOptions } from '@/composeAddressSuggestions'
import type { ComposeIntent } from '@/composeIntent'
import { queryKeys } from '@/queryKeys'
import {
  fetchRuntimeAccounts,
  fetchRuntimeConversationPage,
  fetchRuntimeIdentity,
  fetchRuntimeReplyContext,
  fetchRuntimeSenderAddresses,
} from '@/runtime/adapter'

import { accountFromOptions, wildcardMatchesEmail } from '../composeFormHelpers'

export function useComposeQueries({ intent }: { intent: ComposeIntent }) {
  const identityQuery = useQuery({
    queryKey: ['identity', intent.sourceId],
    queryFn: () => fetchRuntimeIdentity(intent.sourceId),
  })
  const accountsQuery = useQuery({
    queryKey: queryKeys.accounts,
    queryFn: fetchRuntimeAccounts,
  })
  const senderAddressQuery = useQuery({
    queryKey: queryKeys.senderAddresses,
    queryFn: fetchRuntimeSenderAddresses,
  })
  const recipientSuggestionQuery = useQuery({
    queryKey: queryKeys.composeRecipientSuggestions,
    queryFn: () =>
      fetchRuntimeConversationPage({
        limit: 75,
        sort: 'date',
        sortDir: 'desc',
      }),
  })
  const isMessageBasedCompose = intent.kind !== 'new'
  const requiresMessageContext = intent.kind === 'reply'
  const replyContextQuery = useQuery({
    queryKey: requiresMessageContext
      ? ['reply-context', intent.sourceId, intent.messageId]
      : ['reply-context', null],
    queryFn: () =>
      fetchRuntimeReplyContext({
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
    fromOptions,
    identityQuery,
    isMessageBasedCompose,
    recipientSuggestions,
    replyContextQuery,
    requiresMessageContext,
    resolveSubmissionSourceId,
  }
}
