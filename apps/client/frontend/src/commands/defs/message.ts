/**
 * Message action definitions.
 *
 * Pure, role-gated context-menu, palette, header, and keyboard actions
 * expressed as {@link ActionDefinition}s that delegate to `services.email`
 * (never reimplementing the domain logic).
 *
 * Registration order is meaningful: the resolver uses it as the within-section
 * tiebreak, so it mirrors the push order of the two `open` entries, then
 * `toggle-read`/`toggle-flag`, then `archive`, `move-to-inbox`, and the
 * mutually-exclusive trash/delete/discard trio.
 *
 * The two row-scoped `open` / `view-conversation` entries delegate to
 * `services.row` (bound per row by `MessageRow`) with an `services.app`
 * fallback so the palette can surface them too. The `detail-header` surface
 * entries delegate to `services.detail` (bound by `MessageHeader`) or
 * `services.app`, and the draft-vs-message branch is availability-driven (a
 * draft's header resolves to edit/discard only, via {@link notDraftOnHeader}).
 *
 * PARAMETERIZED actions: `move-to-mailbox` (options = the account's mailboxes
 * from `services.mailboxes`, minus the message's current ones and non-movable
 * roles) and `snooze` (options = the header's snooze presets).
 */
import {
  Archive,
  Clock3,
  Eye,
  EyeOff,
  FolderInput,
  Forward,
  Inbox,
  MailOpen,
  MailX,
  Maximize2,
  MessagesSquare,
  Pencil,
  Reply,
  ReplyAll,
  Star,
  Tag,
  Trash2,
} from 'lucide-react'
import type { ListUnsubscribe, MessageDetail } from '../../data/transport/api/index'
import { snoozePresets } from '../../components/mail/detail/snoozePresets'
import { conversationViewQuery } from '../../domain/searchQuery'
import { registerActions } from '../registry'
import type {
  ActionContext,
  ActionDefinition,
  ActionParamOption,
  ActionServices,
  MessageTarget,
} from '../types'

/** Roles from which a message is "removed" and can be restored to the inbox
 *  (mirrors `contextualActions.isRestorableRole`). */
function isRestorableRole(role: string | null): boolean {
  return role === 'trash' || role === 'archive' || role === 'junk'
}

/** The subject a single-target action operates on. `?? undefined` keeps the
 *  accessors total for the empty case. */
function primaryTarget(ctx: ActionContext): MessageTarget | undefined {
  return ctx.targets[0]
}

/** The target's parsed List-Unsubscribe data. Only the DETAIL DTO carries it
 *  (`listUnsubscribe`), so a plain list-row summary yields `undefined` — which
 *  is the availability gate working as designed. */
function unsubscribeTargets(ctx: ActionContext): ListUnsubscribe | undefined {
  const summary = primaryTarget(ctx)?.summary as
    | Partial<Pick<MessageDetail, 'listUnsubscribe'>>
    | undefined
  return summary?.listUnsubscribe ?? undefined
}

/** The subject passed to keyword-toggle handlers. Prefer the summary (carries
 *  the keyword state the handler derives from); fall back to a `MailSelection`
 *  built from the ref so the handler's cache path resolves state. */
function toggleSubject(t: MessageTarget) {
  return t.summary ?? { ...t.ref, conversationId: t.conversationId ?? '' }
}

/** Base enablement shared by every message action: at least one target. In the
 *  context menu a target always exists, so these stay enabled — matching today.
 *  It exists so the palette (later slice) can render "select a message first". */
function requireTarget(ctx: ActionContext) {
  return ctx.targets.length > 0 || { reason: 'Select a message first' }
}

function hasDraftTarget(ctx: ActionContext): boolean {
  return ctx.targets.some((t) => t.isDraft)
}

/** The detail header shows a draft-appropriate action set (edit + discard,
 *  never reply/move/flag). Everywhere else the action keeps its own gating —
 *  the context menu / palette still offer e.g. flagging a draft. */
function notDraftOnHeader(ctx: ActionContext): boolean {
  return !(ctx.surface === 'detail-header' && hasDraftTarget(ctx))
}

/** Mailbox roles a message can NEVER be moved into via "Move to…": drafts/sent
 *  are provider-managed, snooze is scheduler-owned, and trashing has its own
 *  destructive action (with confirm + draft-discard semantics). */
const NON_MOVE_TARGET_ROLES = new Set(['drafts', 'sent', 'snooze', 'trash'])

/** Options for `move-to-mailbox`: the target account's mailboxes (the sidebar's
 *  read model, via `services.mailboxes`), minus the message's current
 *  memberships and the non-movable roles. */
function moveTargetMailboxes(
  ctx: ActionContext,
  services: ActionServices,
): ActionParamOption[] {
  const target = primaryTarget(ctx)
  if (!target || !services.mailboxes) return []
  const current = new Set(target.summary?.mailboxIds ?? [])
  return services.mailboxes
    .list(target.ref.sourceId)
    .filter((mailbox) => !current.has(mailbox.id))
    .filter(
      (mailbox) =>
        mailbox.role === null || !NON_MOVE_TARGET_ROLES.has(mailbox.role),
    )
    .map((mailbox) => ({
      id: mailbox.id,
      label: mailbox.name,
      keywords: mailbox.role ?? undefined,
    }))
}

export const messageActions: readonly ActionDefinition[] = [
  {
    id: 'message.open',
    section: 'open',
    title: 'Open',
    icon: MailOpen,
    keywords: 'open message',
    surfaces: ['context-menu', 'palette'],
    // Row-scoped in the menu (`services.row`, bound by MessageRow); the palette
    // falls back to the app selection handler so "Open" works on the focused
    // message too. Absent both bindings (e.g. the email-only parity harness)
    // the entry stays hidden.
    isAvailable: (_ctx, s) => Boolean(s.row ?? s.app),
    isEnabled: requireTarget,
    run: (ctx, s) => {
      const summary = primaryTarget(ctx)?.summary
      if (!summary) return
      if (s.row) {
        s.row.open(summary)
        return
      }
      s.app?.handleSelectMessage(summary)
    },
  },
  {
    id: 'message.view-conversation',
    section: 'open',
    title: 'View conversation',
    icon: MessagesSquare,
    keywords: 'conversation thread view show',
    // Palette too (owner gap): falls back to the app search handler with the
    // same conversation query the `gc` keyboard goto applies.
    surfaces: ['context-menu', 'palette'],
    isAvailable: (_ctx, s) => Boolean(s.row ?? s.app),
    isEnabled: requireTarget,
    run: (ctx, s) => {
      const target = primaryTarget(ctx)
      if (!target) return
      if (s.row && target.summary) {
        s.row.viewConversation(target.summary)
        return
      }
      if (target.conversationId) {
        s.app?.handleSearch(conversationViewQuery(target.conversationId))
      }
    },
  },
  {
    // "Open message" in its own window — the header's Maximize affordance,
    // also reachable from the palette. The keyboard `o` stays native.
    id: 'message.open-focused',
    section: 'open',
    title: 'Open message',
    icon: Maximize2,
    keywords: 'open message window focus maximize',
    surfaces: ['palette', 'detail-header'],
    isAvailable: (ctx, s) =>
      notDraftOnHeader(ctx) &&
      (ctx.surface === 'detail-header'
        ? Boolean(s.detail?.openFocusedMessage)
        : Boolean(s.app)),
    isEnabled: requireTarget,
    run: (_ctx, s) =>
      (s.detail?.openFocusedMessage ?? s.app?.handleOpenFocusedMessage)?.(),
  },
  {
    id: 'message.toggle-read',
    section: 'state',
    title: (ctx: ActionContext) =>
      primaryTarget(ctx)?.summary?.isRead ? 'Mark unread' : 'Mark read',
    icon: (ctx: ActionContext) =>
      primaryTarget(ctx)?.summary?.isRead ? EyeOff : Eye,
    keywords: 'read unread seen mark',
    surfaces: ['context-menu', 'palette', 'keyboard'],
    shortcut: { key: 'u' },
    isEnabled: requireTarget,
    run: (ctx, s) =>
      ctx.targets.forEach((t) => s.email.toggleRead(toggleSubject(t))),
  },
  {
    id: 'message.toggle-flag',
    section: 'state',
    title: (ctx: ActionContext) =>
      primaryTarget(ctx)?.summary?.isFlagged ? 'Unflag' : 'Flag',
    // Star — the single canonical icon for flag/unflag.
    icon: Star,
    keywords: 'flag unflag star',
    surfaces: ['context-menu', 'palette', 'detail-header'],
    shortcut: { key: 'l', mod: true, shift: true },
    isAvailable: notDraftOnHeader,
    isEnabled: requireTarget,
    run: (ctx, s) =>
      ctx.targets.forEach((t) => s.email.toggleFlag(toggleSubject(t))),
  },
  {
    // Reply: delegates to the header binding when present (works in the focused
    // message window too), else the app handler (palette). The ⌘R chord stays
    // native in dispatch.ts.
    id: 'message.reply',
    section: 'compose-reply',
    title: 'Reply',
    icon: Reply,
    keywords: 'reply respond answer',
    surfaces: ['palette', 'detail-header'],
    shortcut: { key: 'r', mod: true },
    // Never on a draft (you edit a draft, not reply to it).
    isAvailable: (ctx) => !hasDraftTarget(ctx),
    isEnabled: requireTarget,
    run: (_ctx, s) => (s.detail?.reply ?? s.app?.handleReply)?.(),
  },
  {
    id: 'message.reply-all',
    section: 'compose-reply',
    title: 'Reply All',
    icon: ReplyAll,
    keywords: 'reply all respond everyone',
    surfaces: ['palette', 'detail-header'],
    shortcut: { key: 'r', mod: true, shift: true },
    isAvailable: (ctx) => !hasDraftTarget(ctx),
    isEnabled: requireTarget,
    run: (_ctx, s) => (s.detail?.replyAll ?? s.app?.handleReplyAll)?.(),
  },
  {
    id: 'message.forward',
    section: 'compose-reply',
    title: 'Forward',
    icon: Forward,
    keywords: 'forward send along',
    surfaces: ['palette', 'detail-header'],
    isAvailable: (ctx) => !hasDraftTarget(ctx),
    isEnabled: requireTarget,
    run: (_ctx, s) => (s.detail?.forward ?? s.app?.handleForward)?.(),
  },
  {
    // "Edit draft" — availability-driven (every target must be a draft), also
    // reachable from the palette.
    id: 'message.edit-draft',
    section: 'compose-reply',
    title: 'Edit draft',
    icon: Pencil,
    keywords: 'edit draft compose continue',
    surfaces: ['palette', 'detail-header'],
    isAvailable: (ctx, s) =>
      ctx.targets.length > 0 &&
      ctx.targets.every((t) => t.isDraft) &&
      Boolean(s.detail?.editDraft ?? s.app),
    isEnabled: requireTarget,
    run: (_ctx, s) => (s.detail?.editDraft ?? s.app?.handleEditDraft)?.(),
  },
  {
    id: 'message.archive',
    section: 'move',
    title: 'Archive',
    icon: Archive,
    keywords: 'archive',
    surfaces: ['context-menu', 'palette', 'keyboard', 'detail-header'],
    shortcut: { key: 'e' },
    isAvailable: (ctx) =>
      ctx.viewRole !== 'archive' &&
      ctx.viewRole !== 'trash' &&
      notDraftOnHeader(ctx),
    isEnabled: requireTarget,
    run: (ctx, s) => ctx.targets.forEach((t) => s.email.archive(t.ref)),
  },
  {
    id: 'message.move-to-inbox',
    section: 'move',
    title: 'Move to Inbox',
    icon: Inbox,
    keywords: 'move inbox restore',
    surfaces: ['context-menu', 'palette', 'detail-header'],
    isAvailable: (ctx) =>
      isRestorableRole(ctx.viewRole) && notDraftOnHeader(ctx),
    isEnabled: requireTarget,
    run: (ctx, s) => ctx.targets.forEach((t) => s.email.moveToInbox(t.ref)),
  },
  {
    // PARAMETERIZED: "Move to ▸ / Move to…" — the user picks ANY target mailbox
    // of the message's account. Options come from the shared mailbox read model
    // (`services.mailboxes`); the move itself is the same optimistic
    // runtime-mutation path move-to-inbox uses (`services.email.moveToMailbox`).
    id: 'message.move-to-mailbox',
    section: 'move',
    title: 'Move to…',
    icon: FolderInput,
    keywords: 'move mailbox folder file',
    surfaces: ['context-menu', 'palette', 'keyboard'],
    shortcut: { key: 'm' },
    // Hidden wherever no mailbox source is bound (e.g. email-only harnesses);
    // an empty option list is additionally dropped by the resolver.
    isAvailable: (ctx, s) =>
      Boolean(s.mailboxes) && !ctx.targets.some((t) => t.isDraft),
    isEnabled: requireTarget,
    resolveParams: moveTargetMailboxes,
    run: (ctx, s, param) => {
      if (!param) return
      ctx.targets.forEach((t) =>
        s.email.moveToMailbox(t.ref, param.id, param.label),
      )
    },
  },
  {
    id: 'message.move-to-trash',
    section: 'move',
    title: 'Move to Trash',
    icon: Trash2,
    destructive: true,
    keywords: 'trash delete move',
    surfaces: ['context-menu', 'palette', 'keyboard', 'detail-header'],
    // Same chord as delete-permanently below; `isAvailable` disambiguates them
    // (trash-view ⇒ delete-permanently, elsewhere ⇒ this). Stays instant —
    // move-to-trash is reversible via the undo toast.
    shortcut: [{ key: '#' }, { key: 'backspace' }],
    // Not offered in Trash, and never on drafts (a draft is discarded, not
    // trashed).
    isAvailable: (ctx) =>
      ctx.viewRole !== 'trash' && !ctx.targets.some((t) => t.isDraft),
    isEnabled: requireTarget,
    run: (ctx, s) => ctx.targets.forEach((t) => s.email.trash(t.ref)),
  },
  {
    id: 'message.delete-permanently',
    section: 'move',
    title: 'Delete permanently',
    icon: Trash2,
    destructive: true,
    // Irreversible: the keyboard tier PROMPTS via this metadata before running
    // (no silent permanent-delete from a keystroke). The context menu / palette
    // / detail header route through the same gate.
    confirm: {
      title: 'Delete permanently?',
      description: 'This message will be destroyed. This cannot be undone.',
      confirmLabel: 'Delete',
    },
    keywords: 'delete permanently destroy',
    surfaces: ['context-menu', 'palette', 'keyboard', 'detail-header'],
    // Same `#`/Backspace chord as move-to-trash; availability (trash-view only,
    // never a draft) is what makes the resolver pick this one inside Trash.
    shortcut: [{ key: '#' }, { key: 'backspace' }],
    // Trash view only, and never on drafts.
    isAvailable: (ctx) =>
      ctx.viewRole === 'trash' && !ctx.targets.some((t) => t.isDraft),
    isEnabled: requireTarget,
    run: (ctx, s) =>
      ctx.targets.forEach((t) => s.email.deletePermanently(t.ref)),
  },
  {
    id: 'message.discard-draft',
    section: 'move',
    title: 'Discard draft',
    icon: Trash2,
    destructive: true,
    keywords: 'discard draft delete',
    surfaces: ['context-menu', 'palette', 'detail-header'],
    // Every target must be a draft (hard delete via the draft-delete op).
    isAvailable: (ctx) =>
      ctx.targets.length > 0 && ctx.targets.every((t) => t.isDraft),
    isEnabled: requireTarget,
    run: (ctx, s) =>
      ctx.targets.forEach((t) =>
        s.email.discardDraft({ ...t.ref, draftId: t.draftId }),
      ),
  },
  {
    // "Tag" command (folds the old dedicated tagActions provider into the
    // registry): opens the tag editor for the focused message. The app handler
    // already no-ops without a selection; `requireTarget` renders it
    // disabled-with-reason in the palette instead of silently vanishing.
    id: 'message.tag',
    section: 'organize',
    title: 'Tag',
    icon: Tag,
    keywords: 'tag add remove label message',
    surfaces: ['palette', 'keyboard', 'detail-header'],
    shortcut: { key: 't' },
    isAvailable: (ctx, s) =>
      notDraftOnHeader(ctx) &&
      (ctx.surface !== 'detail-header' || Boolean(s.detail?.openTagEditor)),
    isEnabled: requireTarget,
    run: (_ctx, s) =>
      (s.detail?.openTagEditor ?? s.app?.handleOpenTagEditor)?.(),
  },
  {
    // PARAMETERIZED: Snooze with the header's preset options — a REAL command
    // now (it delegates to `email.snooze`, the same mutation the old header
    // popover used) instead of the old palette placeholder toast.
    id: 'message.snooze',
    section: 'organize',
    title: 'Snooze…',
    icon: Clock3,
    keywords: 'snooze later remind',
    surfaces: ['palette', 'detail-header'],
    isAvailable: notDraftOnHeader,
    isEnabled: requireTarget,
    resolveParams: () =>
      snoozePresets().map((preset) => ({
        id: String(preset.until),
        label: preset.label,
      })),
    run: (ctx, s, param) => {
      if (!param) return
      const until = Number(param.id)
      if (!Number.isFinite(until)) return
      ctx.targets.forEach((t) => s.email.snooze(t.ref, until))
    },
  },
  {
    // List-Unsubscribe (RFC 2369/8058). DOUBLY gated: on the parsed targets
    // riding the detail DTO (list summaries never carry them — so the chip
    // appears only on list mail, never as a permanent icon) AND on the
    // `services.unsubscribe` binding, which only hosts whose execution path
    // honors the `confirm` gate may provide (the detail header today) — the
    // one-click POST must never run without its confirmation dialog.
    //
    // Path priority in `run`: one-click (confirmed server-side POST — the
    // RFC 8058 marker means the endpoint acts without a landing page), then
    // mailto (composer prefilled; the user sends), then the plain https link
    // in the system browser. Only the first path is machine-executed, hence
    // only it confirms.
    id: 'message.unsubscribe',
    section: 'organize',
    title: 'Unsubscribe',
    icon: MailX,
    keywords: 'unsubscribe mailing list stop newsletter',
    surfaces: ['context-menu', 'palette', 'detail-header'],
    isAvailable: (ctx, s) =>
      Boolean(s.unsubscribe) &&
      unsubscribeTargets(ctx) !== undefined &&
      !hasDraftTarget(ctx),
    isEnabled: requireTarget,
    confirm: (ctx) => {
      const targets = unsubscribeTargets(ctx)
      if (!targets?.oneClick || !targets.https) {
        // The mailto/browser paths are user-mediated — no dialog.
        return undefined
      }
      const summary = primaryTarget(ctx)?.summary
      const sender = summary?.fromName ?? summary?.fromEmail ?? 'this sender'
      return {
        title: `Unsubscribe from ${sender}?`,
        description:
          'Posthaste will send the standard one-click unsubscribe request to the mailing list on your behalf.',
        confirmLabel: 'Unsubscribe',
      }
    },
    run: (ctx, s) => {
      const target = primaryTarget(ctx)
      const targets = unsubscribeTargets(ctx)
      if (!target || !targets || !s.unsubscribe) return
      if (targets.oneClick && targets.https) {
        return s.unsubscribe.oneClick(target.ref)
      }
      if (targets.mailto) {
        return s.unsubscribe.mailto(targets.mailto)
      }
      if (targets.https) {
        return s.unsubscribe.openLink(targets.https)
      }
    },
  },
]

registerActions(messageActions)
