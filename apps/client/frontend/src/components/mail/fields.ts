/**
 * The message-field registry: one declaration per field a message surface can
 * show, naming it, labelling it, saying which surfaces may show it, and
 * reading its value off a message.
 *
 * It exists because two surfaces render the same underlying fields in
 * different shapes — the message list as resizable columns, the detail pane as
 * labelled rows — and before this they each restated the field set. Adding a
 * field should be one edit here, not one per surface.
 *
 * What is deliberately NOT here is layout. Column width, growth, alignment and
 * cell rendering are the list's business and mean nothing stacked in a detail
 * pane, so `columns.tsx` keeps them and takes only label and value from here.
 *
 * PROMINENCE is here, though, and the distinction is worth stating. No field
 * is privileged in this registry — every one is declared the same way and the
 * reader can turn any of them on, off, or into any order. But fields are not
 * equally important to read, so how loudly a field speaks is a declared
 * PROPERTY of it (`emphasis`, `showLabel`) rather than a branch in the
 * component that draws it. That is the same move the theming rework made for
 * surfaces: roles in the data, not conditionals in the view. It is what lets
 * the subject be as configurable as `Reply-To` while still rendering as the
 * thing your eye lands on.
 */
import type { MessageSummary, Recipient } from '@/data/transport/api'

import { formatRelativeTime } from '@/lib/ambient/time'
import { userTags } from './detail/model'

export type MessageFieldId =
  | 'unread'
  | 'flagged'
  | 'attachment'
  | 'from'
  | 'subject'
  | 'preview'
  | 'date'
  | 'source'
  | 'sourceMailbox'
  | 'tags'
  | 'to'
  | 'cc'
  | 'bcc'
  | 'replyTo'

/** Where a field may be shown. Not every field suits both: read/flag state is
 *  list furniture, and the wider recipient set is detail reading matter. */
export type MessageFieldSurface = 'list' | 'detail'

/**
 * How loudly a detail row speaks, in the three sizes the type scale already
 * has (`--ph-font-size-heading` / `-body` / `-meta`). Not a free size: a row
 * that wants something else wants a scale change, not a fourth name here.
 *
 * The list ignores this — a column's size belongs to the table, which sets one
 * for all of them.
 */
export type MessageFieldEmphasis = 'heading' | 'body' | 'meta'

export interface MessageFieldDef {
  readonly id: MessageFieldId
  /** Human label — the list column header and the detail row's `Label:`. */
  readonly label: string
  readonly surfaces: readonly MessageFieldSurface[]
  /**
   * This message's value as display text, or `''` when it has none.
   *
   * Absent for a field with no context-free textual form: `sourceMailbox`
   * resolves a membership against the mailbox directory, so only the list —
   * which holds that directory — can draw it. Such a field still belongs
   * here for its identity, label and surfaces.
   */
  readonly text?: (message: MessageSummary) => string
  /** Prominence of this field's detail row out of the box; `body` when
   *  unstated. The reader can change it per field. */
  readonly emphasis?: MessageFieldEmphasis
  /** Whether the detail row prints its `Label:` key out of the box; `true`
   *  when unstated. Off suits a field whose value announces itself — a
   *  subject reading `Subject: Re: numbers` is worse than the subject — and
   *  on is essential for one that does not, since a bare list of addresses
   *  cannot say whether it is `To` or `CC`. Hence per field, not global. */
  readonly showLabel?: boolean
}

/**
 * A detail row as the READER has it: which field, how prominent, labelled or
 * not. Position is the array's, so a stored list of these is the whole of
 * "what my message header looks like".
 */
export interface DetailFieldSetting {
  readonly id: MessageFieldId
  readonly emphasis: MessageFieldEmphasis
  readonly showLabel: boolean
}

/** A field's presentation as DECLARED — what a reader gets when they turn it
 *  on, and what "Revert to Default" gives back. */
export function detailFieldDefault(id: MessageFieldId): DetailFieldSetting {
  const field = getMessageField(id)
  return {
    id,
    emphasis: field.emphasis ?? 'body',
    showLabel: field.showLabel ?? true,
  }
}

export function isMessageFieldEmphasis(
  value: unknown,
): value is MessageFieldEmphasis {
  return value === 'heading' || value === 'body' || value === 'meta'
}

/** Joins a recipient list for display. Prefers the display name when the
 *  sender gave one, since `Ada Lovelace` reads better than the raw address,
 *  and falls back to the address when they did not. */
function formatRecipients(recipients: Recipient[] | undefined): string {
  return (recipients ?? [])
    .map((recipient) => recipient.name ?? recipient.email)
    .join(', ')
}

/**
 * Declaration order is picker order — `fieldsForSurface` preserves it, so
 * moving an entry here moves it in the column picker and the detail row list.
 *
 * It is also the ORDER THE DETAIL PANE READS IN, now that the header is
 * nothing but these rows: identity first (what it is, who sent it), then the
 * recipients, then the provenance and the markings. The list's own column
 * order is per-reader and stored, so only its picker follows this.
 */
const FIELDS: readonly MessageFieldDef[] = [
  {
    id: 'unread',
    label: 'Unread',
    surfaces: ['list'],
    text: (message) => (message.isRead ? '' : 'Unread'),
  },
  {
    id: 'flagged',
    label: 'Flag',
    surfaces: ['list'],
    text: (message) => (message.isFlagged ? 'Flagged' : ''),
  },
  // The subject is a field like any other, on both surfaces. The detail pane
  // used to draw it as a heading above the rows — outside the picker and
  // outside this registry — which made "what a message shows" answerable in
  // two places. Its detail row shows the CONVERSATION's subject when there is
  // one; `MessageFieldRows` resolves that fallback before reading fields,
  // since a value function sees one message and not the thread around it.
  // It reads as a heading and prints no key because it used to BE the
  // heading: a `Subject:` in front of it says less than the subject does, and
  // dropping to body size would leave the reading pane with nothing for the
  // eye to land on. Both are defaults the reader can overrule per field.
  {
    id: 'subject',
    label: 'Subject',
    surfaces: ['list', 'detail'],
    text: (message) => message.subject ?? '',
    emphasis: 'heading',
    showLabel: false,
  },
  {
    id: 'from',
    label: 'From',
    surfaces: ['list', 'detail'],
    text: (message) => message.fromName ?? message.fromEmail ?? '',
  },
  {
    id: 'to',
    label: 'To',
    surfaces: ['detail'],
    text: (message) => formatRecipients(message.to),
  },
  // `cc`, `bcc` and `replyTo` are omitted from the wire when empty, hence the
  // `?? []` inside `formatRecipients` — the one place that absence is
  // resolved. `bcc` is empty on essentially all received mail (delivering
  // MTAs strip the header), so it only ever shows on the user's own sent mail
  // and drafts; the empty case must read as "nothing to say", never as broken.
  {
    id: 'cc',
    label: 'CC',
    surfaces: ['detail'],
    text: (message) => formatRecipients(message.cc),
  },
  {
    id: 'bcc',
    label: 'BCC',
    surfaces: ['detail'],
    text: (message) => formatRecipients(message.bcc),
  },
  {
    id: 'replyTo',
    label: 'Reply-To',
    surfaces: ['detail'],
    text: (message) => formatRecipients(message.replyTo),
  },
  {
    id: 'date',
    label: 'Date Received',
    surfaces: ['list'],
    text: (message) => formatRelativeTime(message.receivedAt),
  },
  {
    id: 'source',
    label: 'Account',
    surfaces: ['list', 'detail'],
    text: (message) => message.sourceName,
  },
  { id: 'sourceMailbox', label: 'Mailbox', surfaces: ['list'] },
  // Tags show on both surfaces, and on the detail pane they are CHIPS rather
  // than this text (`MessageFieldRows`) — the header drew them that way before
  // they were fields and there was no reason to demote them to a comma-joined
  // string. The text here is what the list column and any text-shaped consumer
  // read.
  {
    id: 'tags',
    label: 'Tags',
    surfaces: ['list', 'detail'],
    text: (message) => userTags(message.keywords).join(', '),
  },
  // List-only, deliberately: the detail pane renders `MessageAttachments`
  // right below the header, with names and sizes, so a row saying only THAT
  // there are attachments would restate — worse — what is already on screen.
  {
    id: 'attachment',
    label: 'Attachment',
    surfaces: ['list'],
    text: (message) => (message.hasAttachment ? 'Has attachment' : ''),
  },
  {
    id: 'preview',
    label: 'Preview',
    surfaces: ['list'],
    text: (message) => message.preview ?? '',
  },
]

const BY_ID = new Map(FIELDS.map((field) => [field.id, field]))

export function getMessageField(id: MessageFieldId): MessageFieldDef {
  const field = BY_ID.get(id)
  if (!field) {
    throw new Error(`unknown message field: ${id}`)
  }
  return field
}

/** The fields a surface may show, in declaration order. */
export function fieldsForSurface(
  surface: MessageFieldSurface,
): MessageFieldId[] {
  return FIELDS.filter((field) => field.surfaces.includes(surface)).map(
    (field) => field.id,
  )
}

export function isMessageFieldId(id: unknown): id is MessageFieldId {
  return typeof id === 'string' && BY_ID.has(id as MessageFieldId)
}

/** This message's value for a field as text, or `''` when it has none (which
 *  includes a field that has no textual form at all). */
export function messageFieldText(
  id: MessageFieldId,
  message: MessageSummary,
): string {
  return getMessageField(id).text?.(message) ?? ''
}

/** Whether this message has anything to show for a field. The detail pane
 *  renders only present fields, so an absent `CC` is a non-render rather than
 *  an empty row. */
export function hasMessageField(
  id: MessageFieldId,
  message: MessageSummary,
): boolean {
  return messageFieldText(id, message) !== ''
}

/**
 * The detail rows to draw for a message: the reader's selection, IN THE ORDER
 * GIVEN, narrowed to fields the detail pane may show and this message has.
 *
 * Order is the caller's because the reader now owns it — rows are reorderable,
 * so a stored list is a sequence rather than a set, and re-sorting it by
 * declaration here would quietly undo their arrangement. (Declaration order is
 * still what a fresh selection and the pickers use.) Narrowing by presence is
 * what makes an enabled-but-empty field vanish instead of rendering a label
 * with nothing after it — the permanent state of `BCC` on received mail.
 *
 * A repeated id yields one row: stored data is not trusted to be a set.
 */
export function visibleDetailFields(
  selected: Iterable<MessageFieldId>,
  message: MessageSummary,
): MessageFieldId[] {
  const seen = new Set<MessageFieldId>()
  return [...selected].filter((id) => {
    if (seen.has(id) || !isMessageFieldId(id)) return false
    const field = getMessageField(id)
    if (!field.surfaces.includes('detail')) return false
    if (!hasMessageField(id, message)) return false
    seen.add(id)
    return true
  })
}
