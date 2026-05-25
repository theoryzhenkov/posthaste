/**
 * Compose and reply overlay backed by the Rust JMAP send API.
 *
 * @spec docs/L1-ui#component-hierarchy
 * @spec docs/L1-compose#mime-structure
 */
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { ChevronDown, Forward, Loader2, Mail, Reply, Send } from 'lucide-react'
import {
  useCallback,
  useEffect,
  lazy,
  useMemo,
  useRef,
  useState,
  Suspense,
  type SetStateAction,
} from 'react'
import { toast } from 'sonner'

import {
  fetchAccounts,
  fetchConversations,
  fetchIdentity,
  fetchReplyContext,
  fetchSenderAddresses,
  sendMessage,
} from '@/api/client'
import type {
  AccountOverview,
  CachedSenderAddress,
  Recipient,
  SendMessageInput,
} from '@/api/types'
import {
  buildRecipientSuggestionOptions,
  filterAddressSuggestions,
  formatAddressSuggestion,
  insertAddressSuggestion,
  type AddressSuggestionOption,
} from '@/composeAddressSuggestions'
import { cn } from '@/lib/utils'
import { queryKeys } from '@/queryKeys'

import { FloatingPanel } from './FloatingPanel'
import type { MarkdownComposerEditorHandle } from './MarkdownComposerEditor'
import { Button } from './ui/button'
import { Input } from './ui/input'

const MarkdownComposerEditor = lazy(() =>
  import('./MarkdownComposerEditor').then((module) => ({
    default: module.MarkdownComposerEditor,
  })),
)

export type ComposeIntent =
  | { kind: 'new'; sourceId: string }
  | { kind: 'reply'; sourceId: string; messageId: string }
  | { kind: 'forward'; sourceId: string; messageId: string }

interface ComposeOverlayProps {
  intent: ComposeIntent
  onClose: () => void
}

interface ComposeForm {
  from: string
  to: string
  cc: string
  bcc: string
  subject: string
  body: string
}

const EMPTY_FORM: ComposeForm = {
  from: '',
  to: '',
  cc: '',
  bcc: '',
  subject: '',
  body: '',
}

interface FromAddressOption {
  sourceId: string
  sourceName: string
  name: string | null
  email: string
  origin: 'configured' | 'identity' | 'cached'
}

function formatRecipient(recipient: Recipient): string {
  return recipient.name
    ? `${recipient.name} <${recipient.email}>`
    : recipient.email
}

function formatRecipients(recipients: Recipient[]): string {
  return recipients.map(formatRecipient).join(', ')
}

function parseRecipients(value: string): Recipient[] {
  return value
    .split(/[;,]/)
    .map((part) => part.trim())
    .filter(Boolean)
    .map((part) => {
      const match = part.match(/^(.*)<([^>]+)>$/)
      if (!match) {
        return { name: null, email: part }
      }
      const name = match[1].trim().replace(/^"|"$/g, '')
      return {
        name: name || null,
        email: match[2].trim(),
      }
    })
}

function parseSender(value: string): Recipient | null {
  return parseRecipients(value)[0] ?? null
}

function buildSendInput(form: ComposeForm): SendMessageInput {
  return {
    from: parseSender(form.from),
    to: parseRecipients(form.to),
    cc: parseRecipients(form.cc),
    bcc: parseRecipients(form.bcc),
    subject: form.subject.trim(),
    body: form.body,
    inReplyTo: null,
    references: null,
  }
}

function isConcreteEmailPattern(pattern: string): boolean {
  const trimmed = pattern.trim()
  return (
    trimmed.length > 0 &&
    !trimmed.includes('*') &&
    /^[^@\s]+@[^@\s]+$/.test(trimmed)
  )
}

function wildcardMatchesEmail(pattern: string, email: string): boolean {
  const trimmed = pattern.trim().toLowerCase()
  const normalizedEmail = email.trim().toLowerCase()
  return trimmed.startsWith('*@') && normalizedEmail.endsWith(trimmed.slice(1))
}

function optionLabel(option: FromAddressOption): string {
  return option.name ? `${option.name} <${option.email}>` : option.email
}

function accountFromOptions(
  accounts: AccountOverview[],
  identity: Recipient | null,
  identitySourceId: string,
  cachedSenders: CachedSenderAddress[],
): FromAddressOption[] {
  const byAccount = new Map(accounts.map((account) => [account.id, account]))
  const options: FromAddressOption[] = []

  for (const account of accounts) {
    for (const email of account.emailPatterns.filter(isConcreteEmailPattern)) {
      options.push({
        sourceId: account.id,
        sourceName: account.name,
        name: account.fullName,
        email,
        origin: 'configured',
      })
    }
  }

  if (identity) {
    options.unshift({
      sourceId: identitySourceId,
      sourceName: byAccount.get(identitySourceId)?.name ?? identitySourceId,
      name: identity.name,
      email: identity.email,
      origin: 'identity',
    })
  }

  for (const cached of cachedSenders) {
    const account = byAccount.get(cached.sourceId)
    if (!account) {
      continue
    }
    options.push({
      sourceId: cached.sourceId,
      sourceName: account.name,
      name: cached.name,
      email: cached.email,
      origin: 'cached',
    })
  }

  const seen = new Set<string>()
  return options.filter((option) => {
    const key = `${option.sourceId}:${option.email.toLowerCase()}`
    if (seen.has(key)) {
      return false
    }
    seen.add(key)
    return true
  })
}

export function ComposeOverlay({ intent, onClose }: ComposeOverlayProps) {
  const bodyRef = useRef<MarkdownComposerEditorHandle>(null)
  const queryClient = useQueryClient()
  const identityQuery = useQuery({
    queryKey: ['identity', intent.sourceId],
    queryFn: () => fetchIdentity(intent.sourceId),
  })
  const accountsQuery = useQuery({
    queryKey: queryKeys.accounts,
    queryFn: fetchAccounts,
  })
  const senderAddressQuery = useQuery({
    queryKey: queryKeys.senderAddresses,
    queryFn: fetchSenderAddresses,
  })
  const recipientSuggestionQuery = useQuery({
    queryKey: queryKeys.composeRecipientSuggestions,
    queryFn: () =>
      fetchConversations({ limit: 75, sort: 'date', sortDir: 'desc' }),
  })
  const isMessageBasedCompose = intent.kind !== 'new'
  const requiresMessageContext = intent.kind === 'reply'
  const replyContextQuery = useQuery({
    queryKey: requiresMessageContext
      ? ['reply-context', intent.sourceId, intent.messageId]
      : ['reply-context', null],
    queryFn: () =>
      fetchReplyContext(
        intent.sourceId,
        isMessageBasedCompose ? intent.messageId : '',
      ),
    enabled: requiresMessageContext,
  })

  const composeKey = isMessageBasedCompose
    ? `${intent.kind}:${intent.sourceId}:${intent.messageId}`
    : `new:${intent.sourceId}`

  const initialForm = useMemo<ComposeForm>(() => {
    if (intent.kind === 'new' || !replyContextQuery.data) {
      return EMPTY_FORM
    }
    const quoted = replyContextQuery.data.quotedBody
      ? `\n\n${replyContextQuery.data.quotedBody}`
      : ''
    return {
      from: '',
      to:
        intent.kind === 'reply'
          ? formatRecipients(replyContextQuery.data.to)
          : '',
      cc: '',
      bcc: '',
      subject:
        intent.kind === 'reply'
          ? replyContextQuery.data.replySubject
          : replyContextQuery.data.forwardSubject,
      body: quoted,
    }
  }, [intent.kind, replyContextQuery.data])
  const formResetKey = isMessageBasedCompose
    ? `${composeKey}:${replyContextQuery.data ? 'ready' : 'loading'}`
    : composeKey
  const [composeState, setComposeState] = useState(() => ({
    errorMessage: null as string | null,
    form: initialForm,
    resetKey: formResetKey,
  }))
  const [fromMenuOpen, setFromMenuOpen] = useState(false)
  const [fromInputFocused, setFromInputFocused] = useState(false)

  const needsFormReset = composeState.resetKey !== formResetKey
  const form = needsFormReset ? initialForm : composeState.form
  const errorMessage = needsFormReset ? null : composeState.errorMessage
  const setForm = useCallback(
    (nextForm: SetStateAction<ComposeForm>) => {
      setComposeState((current) => {
        const isCurrentForm = current.resetKey === formResetKey
        const baseForm = isCurrentForm ? current.form : initialForm
        return {
          errorMessage: isCurrentForm ? current.errorMessage : null,
          form: typeof nextForm === 'function' ? nextForm(baseForm) : nextForm,
          resetKey: formResetKey,
        }
      })
    },
    [formResetKey, initialForm],
  )
  const setErrorMessage = useCallback(
    (message: string | null) => {
      setComposeState((current) => {
        const isCurrentForm = current.resetKey === formResetKey
        return {
          errorMessage: message,
          form: isCurrentForm ? current.form : initialForm,
          resetKey: formResetKey,
        }
      })
    },
    [formResetKey, initialForm],
  )
  const setField = useCallback(
    <K extends keyof ComposeForm>(field: K, value: ComposeForm[K]) => {
      setForm((current) => ({ ...current, [field]: value }))
    },
    [setForm],
  )
  const handleBodyChange = useCallback(
    (value: string) => setField('body', value),
    [setField],
  )

  useEffect(() => {
    if (isMessageBasedCompose && replyContextQuery.data) {
      requestAnimationFrame(() => bodyRef.current?.focus())
    }
  }, [composeKey, isMessageBasedCompose, replyContextQuery.data])

  useEffect(() => {
    const identity = identityQuery.data
    if (!identity || form.from.trim().length > 0) {
      return
    }
    const frame = requestAnimationFrame(() => {
      setForm((current) =>
        current.from.trim().length > 0
          ? current
          : { ...current, from: formatRecipient(identity) },
      )
    })
    return () => cancelAnimationFrame(frame)
  }, [form.from, identityQuery.data, setForm])

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
  const displayedFromOptions = useMemo(() => {
    const needle = form.from.trim().toLowerCase()
    if (fromMenuOpen || needle.length === 0) {
      return fromOptions
    }
    return fromOptions
      .filter((option) => {
        const label = optionLabel(option).toLowerCase()
        return (
          option.email.toLowerCase().includes(needle) ||
          option.sourceName.toLowerCase().includes(needle) ||
          label.includes(needle)
        )
      })
      .slice(0, 6)
  }, [form.from, fromMenuOpen, fromOptions])

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

  const sendMutation = useMutation({
    mutationFn: (variables: { sourceId: string; input: SendMessageInput }) =>
      sendMessage(variables.sourceId, variables.input),
    onSuccess: async () => {
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: queryKeys.sidebar }),
        queryClient.invalidateQueries({
          queryKey: queryKeys.senderAddresses,
        }),
        queryClient.invalidateQueries({ queryKey: ['conversations'] }),
      ])
      toast('Message sent')
      onClose()
    },
    onError: (error) => {
      setErrorMessage(error.message)
    },
  })

  const isWaitingForMessageContext =
    requiresMessageContext && !replyContextQuery.data
  const isPreparingMessage =
    isWaitingForMessageContext && !replyContextQuery.isError
  const isForwardUnavailable = intent.kind === 'forward'
  const fieldsDisabled = isWaitingForMessageContext || isForwardUnavailable
  const fromLabel = useMemo(() => {
    if (form.from.trim().length > 0) {
      return form.from
    }
    if (identityQuery.isError) {
      return 'Sender unavailable'
    }
    const identity = identityQuery.data
    if (!identity) {
      return 'Loading sender...'
    }
    return identity.name
      ? `${identity.name} <${identity.email}>`
      : identity.email
  }, [form.from, identityQuery.data, identityQuery.isError])

  function validate(
    formData: ComposeForm,
    input: SendMessageInput,
  ): string | null {
    if (
      formData.from.trim().length > 0 &&
      parseRecipients(formData.from).length !== 1
    ) {
      return 'From address must be a single email address.'
    }
    if (!input.from || input.from.email.trim().length === 0) {
      return 'Add a From address.'
    }
    if (input.to.length === 0) {
      return 'Add at least one recipient.'
    }
    if (!isConcreteEmailPattern(input.from.email)) {
      return 'From address must be a single email address.'
    }
    if (input.to.some((recipient) => recipient.email.trim().length === 0)) {
      return 'Recipient email addresses cannot be empty.'
    }
    if (input.subject.length === 0) {
      return 'Add a subject.'
    }
    if (input.body.trim().length === 0) {
      return 'Write a message body.'
    }
    return null
  }

  const handleSubmit = useCallback(() => {
    if (isForwardUnavailable) {
      setErrorMessage(
        'Forward is disabled until forwarded headers and attachments are implemented.',
      )
      return
    }
    if (isWaitingForMessageContext) {
      setErrorMessage('Wait for the message context to finish loading.')
      return
    }

    const input = buildSendInput(form)
    if (intent.kind === 'reply' && replyContextQuery.data) {
      input.inReplyTo = replyContextQuery.data.inReplyTo
      input.references = replyContextQuery.data.references
    }
    const validationError = validate(form, input)
    if (validationError) {
      setErrorMessage(validationError)
      return
    }
    setErrorMessage(null)
    sendMutation.mutate({
      sourceId: resolveSubmissionSourceId(input.from),
      input,
    })
  }, [
    form,
    intent.kind,
    isForwardUnavailable,
    isWaitingForMessageContext,
    replyContextQuery.data,
    resolveSubmissionSourceId,
    sendMutation,
    setErrorMessage,
  ])

  useEffect(() => {
    function handleKeyDown(event: KeyboardEvent) {
      if ((event.metaKey || event.ctrlKey) && event.key === 'Enter') {
        event.preventDefault()
        handleSubmit()
      }
    }

    window.addEventListener('keydown', handleKeyDown)
    return () => window.removeEventListener('keydown', handleKeyDown)
  }, [handleSubmit])

  return (
    <FloatingPanel
      panelLabel={
        intent.kind === 'reply'
          ? 'reply composer'
          : intent.kind === 'forward'
            ? 'forward composer'
            : 'message composer'
      }
      storageKey="posthaste.compose.panelOffset"
      zIndexClassName="z-[80]"
      className="flex h-[min(760px,calc(100vh-40px))] w-[min(860px,calc(100vw-2rem))] flex-col"
      header={
        <div className="flex h-11 min-w-0 items-center gap-2 px-3">
          <div className="flex size-7 shrink-0 items-center justify-center rounded-[7px] bg-[color-mix(in_oklab,var(--brand-coral)_12%,transparent)] text-muted-foreground">
            {intent.kind === 'reply' ? (
              <Reply size={15} />
            ) : intent.kind === 'forward' ? (
              <Forward size={15} />
            ) : (
              <Mail size={15} />
            )}
          </div>
          <div className="min-w-0 flex-1">
            <div className="truncate text-sm font-semibold">
              {intent.kind === 'reply'
                ? 'Reply'
                : intent.kind === 'forward'
                  ? 'Forward'
                  : 'New Message'}
            </div>
            <div className="truncate text-[11px] text-muted-foreground">
              {fromLabel}
            </div>
          </div>
        </div>
      }
      onClose={onClose}
    >
      <div className="grid shrink-0 gap-2 border-b border-border/70 px-4 py-3">
        <ComposeLine label="From">
          <div className="relative flex min-w-0 items-center gap-1">
            <Input
              value={form.from}
              disabled={fieldsDisabled}
              onBlur={() => {
                window.setTimeout(() => {
                  setFromInputFocused(false)
                  setFromMenuOpen(false)
                }, 120)
              }}
              onChange={(event) => {
                setField('from', event.target.value)
                setFromMenuOpen(false)
              }}
              onFocus={() => setFromInputFocused(true)}
              className="h-7 min-w-0 border-border bg-background/45 text-[13px] text-foreground placeholder:text-muted-foreground/70 focus-visible:ring-ring/25"
              placeholder="name@example.com"
            />
            <Button
              type="button"
              variant="ghost"
              size="icon"
              className="size-7 shrink-0 text-muted-foreground hover:bg-[var(--hover-bg)]"
              title="Choose sender"
              disabled={fieldsDisabled}
              onMouseDown={(event) => event.preventDefault()}
              onClick={() => {
                setFromInputFocused(true)
                setFromMenuOpen((open) => !open)
              }}
            >
              <ChevronDown size={15} />
            </Button>
            {(fromMenuOpen || fromInputFocused) &&
              displayedFromOptions.length > 0 && (
                <div className="absolute left-0 right-8 top-8 z-20 max-h-56 overflow-auto rounded-md border border-border bg-popover p-1 text-popover-foreground shadow-lg">
                  {displayedFromOptions.map((option) => (
                    <button
                      key={`${option.sourceId}:${option.email}`}
                      type="button"
                      className="grid w-full min-w-0 grid-cols-[minmax(0,1fr)_auto] items-center gap-2 rounded px-2 py-1.5 text-left text-[12px] hover:bg-[var(--hover-bg)]"
                      onMouseDown={(event) => event.preventDefault()}
                      onClick={() => {
                        setField('from', optionLabel(option))
                        setFromMenuOpen(false)
                        setFromInputFocused(false)
                      }}
                    >
                      <span className="min-w-0 truncate">
                        {optionLabel(option)}
                      </span>
                      <span className="max-w-32 truncate text-[11px] text-muted-foreground">
                        {option.sourceName}
                      </span>
                    </button>
                  ))}
                </div>
              )}
          </div>
        </ComposeLine>
        <ComposeLine label="To">
          <RecipientSuggestionInput
            value={form.to}
            autoFocus={intent.kind === 'new'}
            disabled={fieldsDisabled}
            onChange={(value) => setField('to', value)}
            suggestions={recipientSuggestions}
            placeholder="name@example.com"
          />
        </ComposeLine>
        <ComposeLine label="Cc">
          <RecipientSuggestionInput
            value={form.cc}
            disabled={fieldsDisabled}
            onChange={(value) => setField('cc', value)}
            suggestions={recipientSuggestions}
          />
        </ComposeLine>
        <ComposeLine label="Bcc">
          <RecipientSuggestionInput
            value={form.bcc}
            disabled={fieldsDisabled}
            onChange={(value) => setField('bcc', value)}
            suggestions={recipientSuggestions}
          />
        </ComposeLine>
        <ComposeLine label="Subject">
          <Input
            value={form.subject}
            disabled={fieldsDisabled}
            onChange={(event) => setField('subject', event.target.value)}
            className="h-7 border-border bg-background/45 text-[13px] text-foreground placeholder:text-muted-foreground/70 focus-visible:ring-ring/25"
            placeholder="Subject"
          />
        </ComposeLine>
      </div>

      <div className="min-h-0 flex-1 bg-[color-mix(in_oklab,var(--background)_62%,transparent)]">
        {isForwardUnavailable ? (
          <div className="flex h-full items-center justify-center px-6 text-center text-sm text-muted-foreground">
            Forwarding is disabled until forwarded headers and attachments are
            implemented.
          </div>
        ) : isPreparingMessage ? (
          <div className="flex h-full items-center justify-center gap-2 text-sm text-muted-foreground">
            <Loader2 size={16} className="animate-spin" />
            Preparing reply...
          </div>
        ) : replyContextQuery.isError ? (
          <div className="flex h-full items-center justify-center px-6 text-center text-sm text-destructive">
            Could not prepare this reply. Close the composer and try again.
          </div>
        ) : (
          <Suspense
            fallback={
              <div className="flex h-full items-center justify-center gap-2 text-sm text-muted-foreground">
                <Loader2 size={16} className="animate-spin" />
                Loading editor...
              </div>
            }
          >
            <MarkdownComposerEditor
              ref={bodyRef}
              value={form.body}
              onChange={handleBodyChange}
              placeholder="Write Markdown"
            />
          </Suspense>
        )}
      </div>

      <div className="flex min-h-12 shrink-0 items-center gap-3 border-t border-border/70 px-4 py-2">
        <div
          className={cn(
            'min-w-0 flex-1 truncate text-[12px]',
            errorMessage ? 'text-destructive' : 'text-muted-foreground',
          )}
        >
          {errorMessage ??
            (isForwardUnavailable
              ? 'Forward is not available in this dogfood build.'
              : 'Ready')}
        </div>
        <Button
          type="button"
          variant="outline"
          className="border-border bg-background/45 text-foreground hover:bg-[var(--hover-bg)]"
          onClick={onClose}
        >
          Cancel
        </Button>
        <Button
          type="button"
          onClick={handleSubmit}
          disabled={sendMutation.isPending || fieldsDisabled}
          className="bg-brand-coral text-white hover:bg-brand-coral/90"
        >
          {sendMutation.isPending ? (
            <Loader2 size={15} className="animate-spin" />
          ) : (
            <Send size={15} />
          )}
          Send
        </Button>
      </div>
    </FloatingPanel>
  )
}

function RecipientSuggestionInput({
  autoFocus = false,
  disabled = false,
  onChange,
  placeholder,
  suggestions,
  value,
}: {
  autoFocus?: boolean
  disabled?: boolean
  onChange: (value: string) => void
  placeholder?: string
  suggestions: AddressSuggestionOption[]
  value: string
}) {
  const [focused, setFocused] = useState(false)
  const displayedSuggestions = useMemo(
    () => filterAddressSuggestions(suggestions, value),
    [suggestions, value],
  )

  return (
    <div className="relative">
      <Input
        value={value}
        autoFocus={autoFocus}
        disabled={disabled}
        onBlur={() => {
          window.setTimeout(() => setFocused(false), 120)
        }}
        onChange={(event) => onChange(event.target.value)}
        onFocus={() => setFocused(true)}
        className="h-7 border-border bg-background/45 text-[13px] text-foreground placeholder:text-muted-foreground/70 focus-visible:ring-ring/25"
        placeholder={placeholder}
      />
      {!disabled && focused && displayedSuggestions.length > 0 && (
        <div className="absolute left-0 right-0 top-8 z-20 max-h-56 overflow-auto rounded-md border border-border bg-popover p-1 text-popover-foreground shadow-lg">
          {displayedSuggestions.map((suggestion) => (
            <button
              key={suggestion.email.toLowerCase()}
              type="button"
              className="grid w-full min-w-0 grid-cols-[minmax(0,1fr)_auto] items-center gap-2 rounded px-2 py-1.5 text-left text-[12px] hover:bg-[var(--hover-bg)]"
              onMouseDown={(event) => event.preventDefault()}
              onClick={() => {
                onChange(insertAddressSuggestion(value, suggestion))
                setFocused(false)
              }}
            >
              <span className="min-w-0 truncate">
                {formatAddressSuggestion(suggestion)}
              </span>
              <span className="max-w-32 truncate text-[11px] text-muted-foreground">
                {suggestion.sourceLabel}
              </span>
            </button>
          ))}
        </div>
      )}
    </div>
  )
}

function ComposeLine({
  children,
  label,
}: {
  children: React.ReactNode
  label: string
}) {
  return (
    <label className="grid grid-cols-[4rem_minmax(0,1fr)] items-center gap-2">
      <span className="text-right text-[12px] font-medium text-muted-foreground">
        {label}
      </span>
      {children}
    </label>
  )
}
