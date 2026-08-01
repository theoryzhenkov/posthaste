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
  {
    id: 'attachment',
    label: 'Attachment',
    surfaces: ['list'],
    text: (message) => (message.hasAttachment ? 'Has attachment' : ''),
  },
  {
    id: 'subject',
    label: 'Subject',
    surfaces: ['list'],
    text: (message) => message.subject ?? '',
  },
  {
    id: 'from',
    label: 'From',
    surfaces: ['list', 'detail'],
    text: (message) => message.fromName ?? message.fromEmail ?? '',
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
  {
    id: 'tags',
    label: 'Tags',
    surfaces: ['list'],
    text: (message) => userTags(message.keywords).join(', '),
  },
  {
    id: 'preview',
    label: 'Preview',
    surfaces: ['list'],
    text: (message) => message.preview ?? '',
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
 * The detail rows to draw for a message: the reader's selection, narrowed to
 * the fields this message actually has, in declaration order.
 *
 * Ordering by declaration rather than by selection keeps `To` above `CC`
 * however the reader toggled them on. Narrowing by presence is what makes an
 * enabled-but-empty field vanish instead of rendering a label with nothing
 * after it — the permanent state of `BCC` on received mail.
 */
export function visibleDetailFields(
  selected: Iterable<MessageFieldId>,
  message: MessageSummary,
): MessageFieldId[] {
  const chosen = new Set(selected)
  return FIELDS.filter(
    (field) =>
      field.surfaces.includes('detail') &&
      chosen.has(field.id) &&
      hasMessageField(field.id, message),
  ).map((field) => field.id)
}
