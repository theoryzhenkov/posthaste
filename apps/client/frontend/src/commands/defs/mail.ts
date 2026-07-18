/**
 * Mail-state command definitions: read/flag toggles, archive/move/trash/delete,
 * tagging, snoozing, unsubscribe. Every entry delegates to `services.email`
 * (the domain mutations owning optimistic folds, toasts, and undo) — never
 * reimplementing.
 *
 * Registration order is the resolver's within-section tiebreak: it mirrors the
 * historical order — `toggle-read`/`toggle-flag`, then `archive`,
 * `move-to-inbox`, the parameterized `move-to-mailbox`, and the
 * mutually-exclusive trash/delete/discard trio.
 *
 * PARAMETERIZED actions: `move-to-mailbox` (options = the account's mailboxes
 * from `services.mailboxes`, minus the message's current ones and non-movable
 * roles) and `snooze` (options = the snooze presets, `domain/time`).
 */
import {
  Archive,
  Clock3,
  Eye,
  EyeOff,
  FolderInput,
  Inbox,
  MailX,
  Star,
  Tag,
  Trash2,
} from 'lucide-react'
import { snoozePresets } from '../../domain/time'
import { MAILBOX_ROLES } from '../../domain/vocabulary'
import { registerActions } from '../registry'
import type {
  ActionContext,
  ActionDefinition,
  ActionParamOption,
  ActionServices,
} from '../types'
import {
  hasDraftTarget,
  isRestorableRole,
  notDraftOnHeader,
  primaryTarget,
  requireTarget,
  toggleSubject,
  unsubscribeTargets,
} from './shared'

/** Mailbox roles a message can NEVER be moved into via "Move to…": drafts/sent
 *  are provider-managed, snooze is scheduler-owned, and trashing has its own
 *  destructive action (with confirm + draft-discard semantics). */
const NON_MOVE_TARGET_ROLES = new Set<string>([
  MAILBOX_ROLES.Drafts,
  MAILBOX_ROLES.Sent,
  MAILBOX_ROLES.Snooze,
  MAILBOX_ROLES.Trash,
])

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

const mailActions: readonly ActionDefinition[] = [
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
      ctx.targets.forEach((t) => s.email?.toggleRead(toggleSubject(t))),
  },
  {
    id: 'message.toggle-flag',
    section: 'state',
    title: (ctx: ActionContext) =>
      primaryTarget(ctx)?.summary?.isFlagged ? 'Unflag' : 'Flag',
    // Star — the single canonical icon for flag/unflag.
    icon: Star,
    keywords: 'flag unflag star',
    surfaces: ['context-menu', 'palette', 'detail-header', 'keyboard'],
    // Modifier-chord tier: ⌘⇧L fires even while typing or above an overlay.
    shortcut: {
      key: 'l',
      mod: true,
      shift: true,
      inEditable: true,
      aboveOverlay: true,
    },
    isAvailable: notDraftOnHeader,
    isEnabled: requireTarget,
    run: (ctx, s) =>
      ctx.targets.forEach((t) => s.email?.toggleFlag(toggleSubject(t))),
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
      ctx.viewRole !== MAILBOX_ROLES.Archive &&
      ctx.viewRole !== MAILBOX_ROLES.Trash &&
      notDraftOnHeader(ctx),
    isEnabled: requireTarget,
    run: (ctx, s) => ctx.targets.forEach((t) => s.email?.archive(t.ref)),
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
    run: (ctx, s) => ctx.targets.forEach((t) => s.email?.moveToInbox(t.ref)),
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
        s.email?.moveToMailbox(t.ref, param.id, param.label),
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
      ctx.viewRole !== MAILBOX_ROLES.Trash && !ctx.targets.some((t) => t.isDraft),
    isEnabled: requireTarget,
    run: (ctx, s) => ctx.targets.forEach((t) => s.email?.trash(t.ref)),
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
      ctx.viewRole === MAILBOX_ROLES.Trash && !ctx.targets.some((t) => t.isDraft),
    isEnabled: requireTarget,
    run: (ctx, s) =>
      ctx.targets.forEach((t) => s.email?.deletePermanently(t.ref)),
  },
  {
    id: 'message.discard-draft',
    section: 'move',
    title: 'Discard draft',
    icon: Trash2,
    destructive: true,
    keywords: 'discard draft delete',
    surfaces: ['context-menu', 'palette', 'detail-header', 'keyboard'],
    // Third claimant of the `#`/Backspace chord: on a DRAFT the key discards
    // (hard delete via the draft-delete op) — availability keeps the trio
    // (trash / delete-permanently / discard) mutually exclusive.
    shortcut: [{ key: '#' }, { key: 'backspace' }],
    // Every target must be a draft (hard delete via the draft-delete op).
    isAvailable: (ctx) =>
      ctx.targets.length > 0 && ctx.targets.every((t) => t.isDraft),
    isEnabled: requireTarget,
    run: (ctx, s) =>
      ctx.targets.forEach((t) =>
        s.email?.discardDraft({ ...t.ref, draftId: t.draftId }),
      ),
  },
  {
    // "Tag" command: opens the tag editor for the focused message. The app
    // handler already no-ops without a selection; `requireTarget` renders it
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
    // PARAMETERIZED: Snooze with the preset options (`domain/time`), delegating
    // to `email.snooze` — the same mutation the header popover used.
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
      ctx.targets.forEach((t) => s.email?.snooze(t.ref, until))
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

registerActions(mailActions)
