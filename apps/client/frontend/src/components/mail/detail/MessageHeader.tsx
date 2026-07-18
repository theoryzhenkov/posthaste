/**
 * Message detail header with registry-driven action row.
 *
 * The action row renders from `resolveActions(ctx, { surface: 'detail-header' })`,
 * the same resolver the context menu / palette / keyboard use. This makes the
 * row role-aware and the draft-vs-message branch availability-driven (a draft
 * resolves to edit/discard only).
 *
 * Presentation: icon buttons in a fixed header order, a popover for
 * parameterized actions (Snooze presets), a confirm dialog for destructive
 * `confirm`-bearing actions (delete-permanently), the flag tint, and the tag
 * editor's outside-click anchor attribute.
 */
import { useState, type MouseEvent } from 'react'
import { Paperclip } from 'lucide-react'

import {
  resolveActions,
  runResolvedWithConfirm,
  type ActionConfirm,
  type ActionContext,
  type ActionServices,
  type ResolvedAction,
} from '@/commands'
import { openExternalUrl } from '@/desktop/runtime'
import { SYSTEM_KEYWORDS } from '@/domain/vocabulary'
import type { EmailActions } from '@/data/hooks/useEmailActions'

import type { MessageDetail, MessageSummary } from '@/data/transport/api'

import { Badge } from '../../ui/display/badge'
import { Button } from '../../ui/form/button'
import { KeyboardConfirmDialog } from '../../keyboard/KeyboardConfirmDialog'
import { Popover, PopoverContent, PopoverTrigger } from '../../ui/overlay/popover'
import {
  formatAbsoluteDate,
  formatRecipientEmailList,
  initialsForSender,
  userTags,
} from './model'

export function MessageHeader({
  conversationSubject,
  message,
  actions,
  viewRole,
  onEditDraft,
  onForward,
  onOpenFocusedMessage,
  onReply,
  onReplyAll,
  onSearch,
  onTag,
  onUnsubscribeMailto,
  threadMessages,
}: {
  conversationSubject: string | null | undefined
  message: MessageDetail
  /** Domain mutations the resolved actions delegate to. */
  actions: EmailActions
  /** Role of the current view (null when ambiguous / focused window). */
  viewRole: string | null
  onEditDraft?: () => void
  onForward: () => void
  onOpenFocusedMessage?: () => void
  onReply: () => void
  onReplyAll: () => void
  onSearch?: (query: string, append?: boolean) => void
  onTag?: () => void
  /** Open the composer prefilled from a `mailto:` unsubscribe URI. Hosts
   *  without a composer fall back to the system mailto handler. */
  onUnsubscribeMailto?: (mailtoUri: string) => void
  threadMessages: MessageSummary[]
}) {
  const isDraft = message.keywords.includes(SYSTEM_KEYWORDS.Draft)
  const senderName = message.fromName ?? message.fromEmail ?? 'Unknown sender'
  const senderEmail = message.fromEmail ?? ''
  const tags = userTags(message.keywords)
  const recipientLabel = `to ${formatRecipientEmailList(message.to)}`

  // The header's ActionContext/Services — built per render (cheap plain
  // objects, exactly like MessageRow's). `detail` binds this host's callbacks;
  // absent ones (e.g. no tag editor in the focused window) hide their actions.
  const services: ActionServices = {
    email: actions,
    detail: {
      reply: onReply,
      replyAll: onReplyAll,
      forward: onForward,
      editDraft: onEditDraft,
      openTagEditor: onTag,
      openFocusedMessage: onOpenFocusedMessage,
    },
    // Bound here (and only here) because this host's execution path is
    // `runResolvedWithConfirm` — the one-click POST always gets its dialog.
    unsubscribe: {
      oneClick: (ref) => void actions.unsubscribe(ref),
      mailto: (mailtoUri) =>
        onUnsubscribeMailto
          ? onUnsubscribeMailto(mailtoUri)
          : void openExternalUrl(mailtoUri),
      openLink: (url) => void openExternalUrl(url),
    },
  }
  const actionContext: ActionContext = {
    targets: [
      {
        ref: { sourceId: message.sourceId, messageId: message.id },
        summary: message,
        isDraft,
        draftId: message.draftId,
        conversationId: message.conversationId,
      },
    ],
    viewRole,
    activePane: 'list',
    surface: 'detail-header',
    inputOwner: 'mail',
    hasPendingMutation: actions.isPending,
    connection: 'unknown',
  }
  const headerActions = orderForHeader(resolveActions(actionContext, services))

  return (
    <div className="shrink-0 border-b border-border bg-panel px-5 py-4">
      <div className="flex items-start gap-3">
        <div className="flex size-7 shrink-0 items-center justify-center rounded-full bg-brand-coral text-[11px] font-semibold text-brand-coral-foreground">
          {initialsForSender(senderName)}
        </div>
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-start justify-between gap-3">
            <div className="min-w-0 space-y-1.5">
              <h2 className="text-[17px] font-semibold leading-tight text-foreground">
                {conversationSubject ?? message.subject ?? '(no subject)'}
              </h2>
              <div className="flex flex-wrap items-center gap-x-2 gap-y-1 text-[12px] text-muted-foreground">
                <SenderButtons
                  senderEmail={senderEmail}
                  senderName={senderName}
                  onSearch={onSearch}
                />
                <span className="text-muted-foreground/60">
                  {recipientLabel}
                </span>
                <span className="font-mono text-[11px] text-muted-foreground">
                  {formatAbsoluteDate(message.receivedAt)}
                </span>
                {threadMessages.length > 1 && (
                  <span className="font-mono text-[11px] text-muted-foreground/80">
                    {threadMessages.length} messages
                  </span>
                )}
              </div>
            </div>
            <HeaderActions
              headerActions={headerActions}
              isFlagged={message.isFlagged}
            />
          </div>
          <MessageTagRow
            tags={tags}
            hasAttachment={message.hasAttachment}
            attachmentCount={message.attachments.length}
            onSearch={onSearch}
          />
        </div>
      </div>
    </div>
  )
}

function SenderButtons({
  senderEmail,
  senderName,
  onSearch,
}: {
  senderEmail: string
  senderName: string
  onSearch?: (query: string, append?: boolean) => void
}) {
  return (
    <span className="inline-flex min-w-0 items-center gap-1.5 text-foreground">
      <button
        className="truncate font-medium hover:text-primary hover:underline"
        onClick={(event) =>
          onSearch?.(`from:${senderEmail || senderName}`, event.shiftKey)
        }
        title="Search emails from this sender"
      >
        {senderName}
      </button>
      {senderEmail && senderName !== senderEmail && (
        <button
          className="font-mono text-[11px] text-muted-foreground hover:text-primary hover:underline"
          onClick={(event) => onSearch?.(`from:${senderEmail}`, event.shiftKey)}
          title="Search emails from this sender"
        >
          &lt;{senderEmail}&gt;
        </button>
      )}
    </span>
  )
}

/** The header's fixed visual order (matching the pre-registry layout). Resolver
 *  output is section-ordered for menus; the header keeps its familiar icon-row
 *  arrangement instead. Unknown ids append in resolver order. */
const HEADER_ACTION_ORDER = [
  'message.reply',
  'message.reply-all',
  'message.forward',
  'message.edit-draft',
  'message.archive',
  'message.move-to-inbox',
  'message.move-to-trash',
  'message.delete-permanently',
  'message.discard-draft',
  'message.snooze',
  'message.unsubscribe',
  'message.toggle-flag',
  'message.tag',
  'message.open-focused',
]

function orderForHeader(resolved: ResolvedAction[]): ResolvedAction[] {
  const rank = (action: ResolvedAction) => {
    const index = HEADER_ACTION_ORDER.indexOf(action.def.id)
    return index === -1 ? HEADER_ACTION_ORDER.length : index
  }
  return [...resolved].sort((a, b) => rank(a) - rank(b))
}

/** "Snooze…" → "Snooze": the icon button needs the bare label (and the snooze
 *  e2e flow anchors on `aria-label="Snooze"`). */
function headerLabel(action: ResolvedAction): string {
  return action.title.replace(/…$/, '')
}

function HeaderActions({
  headerActions,
  isFlagged,
}: {
  headerActions: ResolvedAction[]
  isFlagged: boolean
}) {
  // A destructive `confirm`-bearing action (delete-permanently) parks its
  // runner here — same gate the keyboard tier uses, same dialog host.
  const [pendingConfirm, setPendingConfirm] = useState<{
    confirm: ActionConfirm
    onConfirm: () => void
  } | null>(null)

  return (
    <div className="flex shrink-0 items-center gap-1">
      {headerActions.map((action) =>
        action.params ? (
          <HeaderParamAction key={action.def.id} action={action} />
        ) : (
          <Button
            key={action.def.id}
            aria-label={headerLabel(action)}
            data-tag-editor-trigger={
              action.def.id === 'message.tag' ? 'true' : undefined
            }
            onClick={() =>
              runResolvedWithConfirm(action, (confirm, onConfirm) =>
                setPendingConfirm({ confirm, onConfirm }),
              )
            }
            size="icon-sm"
            title={headerLabel(action)}
            type="button"
            variant="ghost"
            className={
              action.def.id === 'message.toggle-flag' && isFlagged
                ? 'text-signal-flag'
                : undefined
            }
          >
            <action.icon size={14} strokeWidth={1.6} />
          </Button>
        ),
      )}
      <KeyboardConfirmDialog
        confirm={pendingConfirm?.confirm ?? null}
        onConfirm={() => {
          pendingConfirm?.onConfirm()
          setPendingConfirm(null)
        }}
        onCancel={() => setPendingConfirm(null)}
      />
    </div>
  )
}

/** A PARAMETERIZED action in the header renders as an icon button + popover of
 *  its options (the Snooze presets popover, generically). */
function HeaderParamAction({ action }: { action: ResolvedAction }) {
  const [open, setOpen] = useState(false)
  const label = headerLabel(action)
  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <Button
          aria-label={label}
          size="icon-sm"
          title={label}
          type="button"
          variant="ghost"
        >
          <action.icon size={14} strokeWidth={1.6} />
        </Button>
      </PopoverTrigger>
      <PopoverContent align="end" className="w-44">
        {(action.params ?? []).map((option) => (
          <Button
            key={option.id}
            className="w-full justify-start"
            onClick={() => {
              void action.executeWith?.(option)
              setOpen(false)
            }}
            size="sm"
            type="button"
            variant="ghost"
          >
            {option.label}
          </Button>
        ))}
      </PopoverContent>
    </Popover>
  )
}

function MessageTagRow({
  attachmentCount,
  hasAttachment,
  tags,
  onSearch,
}: {
  attachmentCount: number
  hasAttachment: boolean
  tags: string[]
  onSearch?: (query: string, append?: boolean) => void
}) {
  if (tags.length === 0 && !hasAttachment && attachmentCount === 0) {
    return null
  }
  return (
    <div className="mt-3 flex flex-wrap items-center gap-2">
      {tags.map((tag) => (
        <Badge
          variant="outline"
          className="cursor-pointer rounded-[4px] border-border/80 bg-background/45 px-1.5 py-0.5 font-mono text-[10px] uppercase text-muted-foreground hover:border-primary hover:text-primary"
          key={tag}
          onClick={(event: MouseEvent) =>
            onSearch?.(`tag:${tag}`, event.shiftKey)
          }
          title={`Search emails tagged "${tag}"`}
        >
          {tag}
        </Badge>
      ))}
      {hasAttachment && (
        <button
          className="inline-flex items-center gap-1.5 rounded-[4px] border border-border/80 bg-background/45 px-2 py-0.5 text-[11px] font-medium text-muted-foreground transition-colors hover:border-primary hover:text-primary"
          onClick={(event) => onSearch?.('has:attachment', event.shiftKey)}
          title="Search emails with attachments"
        >
          <Paperclip size={12} strokeWidth={1.6} />
          Has attachment
        </button>
      )}
    </div>
  )
}
