import { useCallback, useMemo } from 'react'
import { useQueries, useQuery } from '@tanstack/react-query'

import type { Identity, Recipient } from '@/data/transport/api'
import {
  parseEmailAddress,
  parseEmailPattern,
  patternEmailAddress,
  patternMatchesEmail,
  type EmailAddress,
  type AddressSuggestionOption,
} from '@/domain/address'
import type { ComposeIntent } from '@/domain/composeIntent'
import {
  fetchQuery,
  useAccountSettings,
  useAccounts,
  useMailClient,
  useSenderAddresses,
} from '@/data'
import { familyKey, queryKeys } from '@/data/queries/queryKeys'
import type { AccountSettingsResult, MessageDetailResult } from '@/gen'

import {
  accountFromOptions,
  replyContextFromDetail,
  type ComposeAccount,
  type ComposeSenderAddress,
} from '../form/model'

export function useComposeQueries({ intent }: { intent: ComposeIntent }) {
  const client = useMailClient()
  // Identity + signature + email patterns live on the per-account
  // `accountSettings` family; the `accounts` family carries the row list the
  // From options span.
  const accountsQuery = useAccounts()
  const accountRows = useMemo(
    () => accountsQuery.data?.rows ?? [],
    [accountsQuery.data],
  )
  const settingsByAccount = useQueries({
    queries: accountRows.map((row) => ({
      queryKey: familyKey({ accountSettings: { accountId: row.id } }),
      queryFn: () =>
        fetchQuery<AccountSettingsResult>(client, {
          accountSettings: { accountId: row.id },
        }),
    })),
    combine: (results) => {
      const byId = new Map<string, AccountSettingsResult>()
      for (const result of results) {
        if (result.data) {
          byId.set(result.data.id, result.data)
        }
      }
      return byId
    },
  })
  const composeAccounts = useMemo<ComposeAccount[]>(
    () =>
      accountRows.map((row) => ({
        id: row.id,
        name: row.name,
        fullName: row.fullName,
        // The parse boundary for account sending patterns: raw wire strings
        // become EmailPattern here; junk patterns drop out once.
        emailPatterns: (settingsByAccount.get(row.id)?.emailPatterns ?? [])
          .map(parseEmailPattern)
          .filter((pattern): pattern is NonNullable<typeof pattern> =>
            pattern !== null,
          ),
      })),
    [accountRows, settingsByAccount],
  )

  // The composing account's sending identity: its display name plus the first
  // concrete address among its email patterns.
  const identityQuery = useAccountSettings(intent.sourceId)
  const identity = useMemo<Identity | undefined>(() => {
    const settings = identityQuery.data
    if (!settings) {
      return undefined
    }
    const email =
      settings.emailPatterns
        .map(parseEmailAddress)
        .find((candidate): candidate is EmailAddress => candidate !== null) ??
      null
    if (!email) {
      return undefined
    }
    return {
      id: settings.id,
      name: settings.fullName ?? settings.name,
      email,
    }
  }, [identityQuery.data])
  const signature = identityQuery.data?.signature ?? null

  const senderAddressesQuery = useSenderAddresses()
  const cachedSenders = useMemo<ComposeSenderAddress[]>(
    () =>
      (senderAddressesQuery.data?.rows ?? []).map((row) => ({
        sourceId: row.accountId,
        name: row.name,
        email: row.email,
      })),
    [senderAddressesQuery.data],
  )

  // The provider message this compose is anchored to (reply/reply-all/forward
  // quote context, or the draft being resumed); null for the from-scratch kinds
  // (`new`, `mailto`), which carry no messageId.
  const anchoredMessageId =
    intent.kind === 'new' || intent.kind === 'mailto' ? null : intent.messageId
  const isMessageBasedCompose = anchoredMessageId !== null
  const requiresMessageContext =
    intent.kind === 'reply' ||
    intent.kind === 'replyAll' ||
    intent.kind === 'forward'
  const isDraftEdit = intent.kind === 'draft'

  // A draft resume seeds the form from the draft message's detail answer.
  // The detail projection carries From/To/subject/body; Cc/Bcc are not
  // served, so a resumed draft cannot restore them.
  const draftSeedQuery = useQuery({
    queryKey: isDraftEdit
      ? queryKeys.messageDetail({
          accountId: intent.sourceId,
          messageId: intent.messageId,
        })
      : ['messageDetail', 'draft-seed-none'],
    queryFn: () =>
      fetchQuery<MessageDetailResult>(client, {
        messageDetail: {
          accountId: intent.sourceId,
          messageId: isDraftEdit ? intent.messageId : '',
        },
      }),
    enabled: isDraftEdit,
  })
  const draftSeed = useMemo(() => {
    const detail = draftSeedQuery.data
    if (!detail) {
      return undefined
    }
    const summary = detail.summary
    return {
      from: summary.fromEmail
        ? summary.fromName
          ? `${summary.fromName} <${summary.fromEmail}>`
          : summary.fromEmail
        : '',
      to: summary.to
        .map((recipient) =>
          recipient.name
            ? `${recipient.name} <${recipient.email}>`
            : recipient.email,
        )
        .join(', '),
      cc: '',
      bcc: '',
      subject: summary.subject ?? '',
      body: detail.bodyText ?? '',
    }
  }, [draftSeedQuery.data])

  // The reply/forward context is derived client-side from the anchored
  // message's `messageDetail` answer (threading headers on the summary, the
  // quote from the inline body). The family key is shared with the reader
  // pane, so a message the user just had open seeds the quote instantly.
  const replyContextQuery = useQuery({
    queryKey: requiresMessageContext
      ? queryKeys.messageDetail({
          accountId: intent.sourceId,
          messageId: anchoredMessageId ?? '',
        })
      : ['messageDetail', 'reply-context-none'],
    queryFn: () =>
      fetchQuery<MessageDetailResult>(client, {
        messageDetail: {
          accountId: intent.sourceId,
          messageId: anchoredMessageId ?? '',
        },
      }),
    select: replyContextFromDetail,
    enabled: requiresMessageContext,
  })

  const composeKey =
    anchoredMessageId !== null
      ? `${intent.kind}:${intent.sourceId}:${anchoredMessageId}`
      : intent.kind === 'mailto'
        ? // Keyed by the URI so opening a different mailto reseeds the form.
          `mailto:${intent.sourceId}:${intent.mailtoUri}`
        : `new:${intent.sourceId}`

  const fromIdentity = useMemo(
    () =>
      identity
        ? {
            name: identity.name || null,
            email: identity.email,
          }
        : null,
    [identity],
  )
  const fromOptions = useMemo(
    () =>
      accountFromOptions(
        composeAccounts,
        fromIdentity,
        intent.sourceId,
        cachedSenders,
      ),
    [composeAccounts, fromIdentity, intent.sourceId, cachedSenders],
  )
  // Recipient autocomplete: the accounts' own concrete addresses first, then
  // the server-side address book (`senderAddresses`), de-duplicated by email.
  const recipientSuggestions = useMemo<AddressSuggestionOption[]>(() => {
    const options: AddressSuggestionOption[] = []
    for (const account of composeAccounts) {
      for (const email of account.emailPatterns.flatMap(
        (pattern) => patternEmailAddress(pattern) ?? [],
      )) {
        options.push({
          name: account.fullName,
          email,
          sourceLabel: account.name,
          origin: 'account',
        })
      }
    }
    const accountNameById = new Map(
      composeAccounts.map((account) => [account.id, account.name]),
    )
    for (const sender of cachedSenders) {
      const email = parseEmailAddress(sender.email)
      if (!email) {
        continue
      }
      options.push({
        name: sender.name,
        email,
        sourceLabel: accountNameById.get(sender.sourceId) ?? 'Address book',
        origin: 'correspondent',
      })
    }
    const seen = new Set<string>()
    return options.filter((option) => {
      const key = option.email.trim().toLowerCase()
      if (!key || seen.has(key)) {
        return false
      }
      seen.add(key)
      return true
    })
  }, [composeAccounts, cachedSenders])

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
      const currentAccount = composeAccounts.find(
        (account) => account.id === intent.sourceId,
      )
      if (
        currentAccount?.emailPatterns.some((pattern) =>
          patternMatchesEmail(pattern, email),
        )
      ) {
        return currentAccount.id
      }
      return (
        composeAccounts.find((account) =>
          account.emailPatterns.some((pattern) =>
            patternMatchesEmail(pattern, email),
          ),
        )?.id ?? intent.sourceId
      )
    },
    [composeAccounts, fromOptions, intent.sourceId],
  )

  return {
    composeKey,
    draftSeed,
    // Stable identity of the resumed draft, once its content loads. Saves key
    // by this so an edit updates the draft in place across provider id
    // rotation; `null` for a draft row without one (keyed by id instead).
    draftSeedDraftId: draftSeedQuery.data?.summary.draftId ?? null,
    draftSeedQuery,
    fromOptions,
    identity,
    identityQuery,
    isDraftEdit,
    isMessageBasedCompose,
    recipientSuggestions,
    replyContextQuery,
    requiresMessageContext,
    resolveSubmissionSourceId,
    signature,
  }
}
