/**
 * Host-side binding builders: turn a host's callbacks + read models into the
 * `(ctx, services)` pair and resolve, yielding the flattened
 * `ResolvedActionView[]` components render (`lib/command`).
 *
 * These are the ONLY constructors of the detail-header and row-context-menu
 * resolutions — both hosts of each surface (mail shell + focused window;
 * every row) flow through here, so the context shape can never drift between
 * them. `app/` calls these and passes the resulting closures down as props;
 * components never import `commands/` (R11).
 */
import type {
  Mailbox,
  MessageDetail,
  MessageSummary,
} from '../data/transport/api/index'
import type { EmailActions } from '../data/hooks/useEmailActions'
import { SYSTEM_KEYWORDS } from '../domain/vocabulary'
import { resolveActions, type ResolvedAction } from './resolve'
import type { ActionContext, ActionServices } from './types'

export interface DetailHeaderBinding {
  /** Domain mutations the resolved actions delegate to. */
  email: EmailActions
  /** Role of the current view (null when ambiguous / focused window). */
  viewRole: string | null
  detail: {
    reply: () => void
    replyAll: () => void
    forward: () => void
    editDraft?: () => void
    openTagEditor?: () => void
    openFocusedMessage?: () => void
  }
  /** Open the composer prefilled from a `mailto:` unsubscribe URI; hosts
   *  without a composer omit it and the system mailto handler takes over. */
  unsubscribeMailto?: (mailtoUri: string) => void
  /** System-browser opener (desktop runtime — injected, R11). */
  openExternalUrl: (url: string) => void | Promise<void>
}

/**
 * The detail header's action row for one loaded message. Bound here (and only
 * here) because the header's execution path is `runActionWithConfirm` — the
 * unsubscribe one-click POST always gets its dialog.
 */
export function buildDetailHeaderActions(
  binding: DetailHeaderBinding,
): (message: MessageDetail) => ResolvedAction[] {
  const services: ActionServices = {
    email: binding.email,
    detail: binding.detail,
    unsubscribe: {
      oneClick: (ref) => void binding.email.unsubscribe(ref),
      mailto: (mailtoUri) =>
        binding.unsubscribeMailto
          ? binding.unsubscribeMailto(mailtoUri)
          : void binding.openExternalUrl(mailtoUri),
      openLink: (url) => void binding.openExternalUrl(url),
    },
  }
  return (message) => {
    const ctx: ActionContext = {
      targets: [
        {
          ref: { sourceId: message.sourceId, messageId: message.id },
          summary: message,
          isDraft: message.keywords.includes(SYSTEM_KEYWORDS.Draft),
          draftId: message.draftId,
          conversationId: message.conversationId,
        },
      ],
      viewRole: binding.viewRole,
      activePane: 'list',
      surface: 'detail-header',
      inputOwner: 'mail',
      hasPendingMutation: binding.email.isPending,
      connection: 'unknown',
    }
    return resolveActions(ctx, services)
  }
}

export interface RowContextMenuBinding {
  email: EmailActions
  /** Role of the current view, deriving contextual actions; null = ambiguous. */
  viewRole: string | null
}

/** The per-row capabilities a message row supplies at menu time: its own
 *  open/view callbacks and the account's cache-only mailbox read model. */
export interface RowContextMenuInput {
  message: MessageSummary
  open: (message: MessageSummary) => void
  viewConversation: (message: MessageSummary) => void
  mailboxes: { list: (sourceId: string) => Mailbox[] }
}

/** A message row's context menu: `services.row` binds the two `open` entries
 *  to the row callbacks; `mailboxes` feeds the parameterized "Move to ▸". */
export function buildRowContextMenu(
  binding: RowContextMenuBinding,
): (input: RowContextMenuInput) => ResolvedAction[] {
  return (input) => {
    const services: ActionServices = {
      email: binding.email,
      row: { open: input.open, viewConversation: input.viewConversation },
      mailboxes: input.mailboxes,
    }
    const ctx: ActionContext = {
      targets: [
        {
          ref: {
            messageId: input.message.id,
            sourceId: input.message.sourceId,
          },
          summary: input.message,
          isDraft: input.message.keywords.includes(SYSTEM_KEYWORDS.Draft),
          draftId: input.message.draftId,
          conversationId: input.message.conversationId,
        },
      ],
      viewRole: binding.viewRole,
      activePane: 'list',
      surface: 'context-menu',
      inputOwner: 'mail',
      hasPendingMutation: binding.email.isPending,
      connection: 'unknown',
    }
    return resolveActions(ctx, services)
  }
}
