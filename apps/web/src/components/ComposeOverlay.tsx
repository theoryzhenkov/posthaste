/**
 * Compose and reply overlay backed by the Rust JMAP send API.
 *
 * @spec docs/L1-ui#component-hierarchy
 * @spec docs/L1-compose#mime-structure
 */
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { Loader2, Mail, Reply, Send } from 'lucide-react'
import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type SetStateAction,
} from 'react'
import { toast } from 'sonner'

import { fetchIdentity, fetchReplyContext, sendMessage } from '@/api/client'
import type { Recipient, SendMessageInput } from '@/api/types'
import { cn } from '@/lib/utils'
import { queryKeys } from '@/queryKeys'

import { FloatingPanel } from './FloatingPanel'
import { Button } from './ui/button'
import { Input } from './ui/input'

export type ComposeIntent =
  | { kind: 'new'; sourceId: string }
  | { kind: 'reply'; sourceId: string; messageId: string }

interface ComposeOverlayProps {
  intent: ComposeIntent
  onClose: () => void
}

interface ComposeForm {
  to: string
  cc: string
  bcc: string
  subject: string
  body: string
}

const EMPTY_FORM: ComposeForm = {
  to: '',
  cc: '',
  bcc: '',
  subject: '',
  body: '',
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

function buildSendInput(form: ComposeForm): SendMessageInput {
  return {
    to: parseRecipients(form.to),
    cc: parseRecipients(form.cc),
    bcc: parseRecipients(form.bcc),
    subject: form.subject.trim(),
    body: form.body,
    inReplyTo: null,
    references: null,
  }
}

export function ComposeOverlay({ intent, onClose }: ComposeOverlayProps) {
  const bodyRef = useRef<HTMLTextAreaElement>(null)
  const queryClient = useQueryClient()
  const identityQuery = useQuery({
    queryKey: ['identity', intent.sourceId],
    queryFn: () => fetchIdentity(intent.sourceId),
  })
  const replyContextQuery = useQuery({
    queryKey:
      intent.kind === 'reply'
        ? ['reply-context', intent.sourceId, intent.messageId]
        : ['reply-context', null],
    queryFn: () =>
      fetchReplyContext(
        intent.sourceId,
        intent.kind === 'reply' ? intent.messageId : '',
      ),
    enabled: intent.kind === 'reply',
  })

  const composeKey =
    intent.kind === 'reply'
      ? `${intent.sourceId}:${intent.messageId}`
      : intent.sourceId

  const initialForm = useMemo<ComposeForm>(() => {
    if (intent.kind === 'new') {
      return EMPTY_FORM
    }
    if (!replyContextQuery.data) {
      return EMPTY_FORM
    }
    const quoted = replyContextQuery.data.quotedBody
      ? `\n\n${replyContextQuery.data.quotedBody}`
      : ''
    return {
      to: formatRecipients(replyContextQuery.data.to),
      cc: '',
      bcc: '',
      subject: replyContextQuery.data.replySubject,
      body: quoted,
    }
  }, [intent.kind, replyContextQuery.data])
  const formResetKey =
    intent.kind === 'reply'
      ? `${composeKey}:${replyContextQuery.data ? 'ready' : 'loading'}`
      : composeKey
  const [composeState, setComposeState] = useState(() => ({
    errorMessage: null as string | null,
    form: initialForm,
    resetKey: formResetKey,
  }))

  if (composeState.resetKey !== formResetKey) {
    setComposeState({
      errorMessage: null,
      form: initialForm,
      resetKey: formResetKey,
    })
  }

  const form =
    composeState.resetKey === formResetKey ? composeState.form : initialForm
  const errorMessage =
    composeState.resetKey === formResetKey ? composeState.errorMessage : null
  const setForm = useCallback((nextForm: SetStateAction<ComposeForm>) => {
    setComposeState((current) => ({
      ...current,
      form: typeof nextForm === 'function' ? nextForm(current.form) : nextForm,
    }))
  }, [])
  const setErrorMessage = useCallback((message: string | null) => {
    setComposeState((current) => ({
      ...current,
      errorMessage: message,
    }))
  }, [])

  useEffect(() => {
    if (intent.kind === 'reply' && replyContextQuery.data) {
      requestAnimationFrame(() => bodyRef.current?.focus())
    }
  }, [composeKey, intent.kind, replyContextQuery.data])

  const sendMutation = useMutation({
    mutationFn: (input: SendMessageInput) =>
      sendMessage(intent.sourceId, input),
    onSuccess: async () => {
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: queryKeys.sidebar }),
        queryClient.invalidateQueries({ queryKey: ['conversations'] }),
      ])
      toast('Message sent')
      onClose()
    },
    onError: (error) => {
      setErrorMessage(error.message)
    },
  })

  const isPreparingReply =
    intent.kind === 'reply' && replyContextQuery.isLoading
  const fromLabel = useMemo(() => {
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
  }, [identityQuery.data, identityQuery.isError])

  function setField<K extends keyof ComposeForm>(
    field: K,
    value: ComposeForm[K],
  ) {
    setForm((current) => ({ ...current, [field]: value }))
  }

  function validate(input: SendMessageInput): string | null {
    if (input.to.length === 0) {
      return 'Add at least one recipient.'
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
    const input = buildSendInput(form)
    if (intent.kind === 'reply' && replyContextQuery.data) {
      input.inReplyTo = replyContextQuery.data.inReplyTo
      input.references = replyContextQuery.data.references
    }
    const validationError = validate(input)
    if (validationError) {
      setErrorMessage(validationError)
      return
    }
    setErrorMessage(null)
    sendMutation.mutate(input)
  }, [form, intent.kind, replyContextQuery.data, sendMutation, setErrorMessage])

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
        intent.kind === 'reply' ? 'reply composer' : 'message composer'
      }
      storageKey="posthaste.compose.panelOffset"
      zIndexClassName="z-[80]"
      className="flex h-[min(760px,calc(100vh-40px))] max-w-[860px] flex-col"
      header={
        <div className="flex h-11 min-w-0 items-center gap-2 px-3">
          <div className="flex size-7 shrink-0 items-center justify-center rounded-[7px] bg-[color-mix(in_oklab,var(--brand-coral)_12%,transparent)] text-muted-foreground">
            {intent.kind === 'reply' ? <Reply size={15} /> : <Mail size={15} />}
          </div>
          <div className="min-w-0 flex-1">
            <div className="truncate text-sm font-semibold">
              {intent.kind === 'reply' ? 'Reply' : 'New Message'}
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
        <ComposeLine label="To">
          <Input
            value={form.to}
            autoFocus={intent.kind === 'new'}
            onChange={(event) => setField('to', event.target.value)}
            className="h-7 border-border bg-background/45 text-[13px] text-foreground placeholder:text-muted-foreground/70 focus-visible:ring-ring/25"
            placeholder="name@example.com"
          />
        </ComposeLine>
        <ComposeLine label="Cc">
          <Input
            value={form.cc}
            onChange={(event) => setField('cc', event.target.value)}
            className="h-7 border-border bg-background/45 text-[13px] text-foreground placeholder:text-muted-foreground/70 focus-visible:ring-ring/25"
          />
        </ComposeLine>
        <ComposeLine label="Bcc">
          <Input
            value={form.bcc}
            onChange={(event) => setField('bcc', event.target.value)}
            className="h-7 border-border bg-background/45 text-[13px] text-foreground placeholder:text-muted-foreground/70 focus-visible:ring-ring/25"
          />
        </ComposeLine>
        <ComposeLine label="Subject">
          <Input
            value={form.subject}
            onChange={(event) => setField('subject', event.target.value)}
            className="h-7 border-border bg-background/45 text-[13px] text-foreground placeholder:text-muted-foreground/70 focus-visible:ring-ring/25"
            placeholder="Subject"
          />
        </ComposeLine>
      </div>

      <div className="min-h-0 flex-1 bg-[color-mix(in_oklab,var(--background)_62%,transparent)]">
        {isPreparingReply ? (
          <div className="flex h-full items-center justify-center gap-2 text-sm text-muted-foreground">
            <Loader2 size={16} className="animate-spin" />
            Preparing reply...
          </div>
        ) : (
          <textarea
            ref={bodyRef}
            value={form.body}
            onChange={(event) => setField('body', event.target.value)}
            className="ph-scroll h-full w-full resize-none bg-transparent px-5 py-4 font-mono text-[13px] leading-6 text-foreground outline-none placeholder:text-muted-foreground/70"
            placeholder="Message"
            spellCheck
          />
        )}
      </div>

      <div className="flex min-h-12 shrink-0 items-center gap-3 border-t border-border/70 px-4 py-2">
        <div
          className={cn(
            'min-w-0 flex-1 truncate text-[12px]',
            errorMessage ? 'text-destructive' : 'text-muted-foreground',
          )}
        >
          {errorMessage ?? 'Ready'}
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
          disabled={sendMutation.isPending || isPreparingReply}
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
