/**
 * The detail pane's field rows — the subject, `From:`, `To:`, `Arrived at:`,
 * and whichever others the reader has turned on, each as loud and as labelled
 * as they have asked for — plus the picker that chooses them.
 *
 * These rows are now the WHOLE message header. The subject heading and the
 * sender line used to sit above them, drawn by `MessageHeader` and answerable
 * to nothing: they could not be turned off, reordered, or reasoned about
 * alongside the fields below them. Folding them in makes one list, one
 * picker, one registry — and one reason the defaults must name `subject` and
 * `from` (`useFieldConfig`), since nothing else draws them now.
 *
 * Two rules govern what appears. A field must be SELECTED — which carries its
 * place in the order and how it presents — and it must be PRESENT on this
 * message. The second is why an empty field is a genuine
 * non-render rather than a row with nothing after the colon: `BCC` is stripped
 * in transit on every received message, so a reader who turns it on would
 * otherwise see a permanently broken-looking row on all their mail.
 *
 * The picker is reachable two ways here — right-click anywhere on the rows, or
 * the button beside them — and a third time in Settings › Appearance, which
 * configures these rows and the list's columns together.
 */
import type { MouseEvent } from 'react'

import type { MessageSummary } from '@/data/transport/api'
import { cn } from '@/lib/design/cn'

import { Badge } from '../../ui/display/badge'
import {
  fieldsForSurface,
  getMessageField,
  messageFieldText,
  visibleDetailFields,
  type MessageFieldEmphasis,
  type MessageFieldId,
} from '../fields'
import {
  FieldPickerButton,
  FieldPickerMenu,
  fieldPickerOptions,
} from '../thread/fieldPicker'
import { useDetailFieldConfig } from '../thread/useFieldConfig'
import { formatAbsoluteDate, userTags } from './model'

/** Every field the picker may offer here, in declaration order. What actually
 *  renders, and in what order, is the reader's stored arrangement. */
const DETAIL_FIELDS = fieldsForSurface('detail')

/**
 * Prominence, in the three sizes the type scale has. Weight and colour ride
 * along with size because prominence is the point: a 17px row in muted grey
 * would not be a heading, it would be a large aside.
 */
const EMPHASIS_CLASS: Record<MessageFieldEmphasis, string> = {
  heading: 'text-heading font-semibold leading-tight text-foreground',
  body: 'text-body text-foreground/85',
  meta: 'text-meta text-muted-foreground',
}

export function MessageFieldRows({
  conversationSubject,
  message,
  onSearch,
  threadMessageCount,
}: {
  /** The thread's subject, which wins over this message's own (replies
   *  restate it, often with `Re:` bolted on). It is a property of the
   *  CONVERSATION, so it cannot come off a registry value function — those
   *  see one message. Resolving it into the message the rows read keeps the
   *  fallback chain in one place and lets `subject` stay an ordinary field
   *  everywhere else. */
  conversationSubject?: string | null
  message: MessageSummary
  onSearch?: (query: string, append?: boolean) => void
  /** Shown alongside the date when the conversation has more than one
   *  message; not a message field, so it is not in the registry. */
  threadMessageCount: number
}) {
  const { detailFields, toggleDetailField, resetDetailFields } =
    useDetailFieldConfig()

  // `(no subject)` is a real value rather than an absence: a subject-less
  // message should say so, not silently lose the row it would have had.
  const resolved: MessageSummary = {
    ...message,
    subject: conversationSubject ?? message.subject ?? '(no subject)',
  }

  const options = fieldPickerOptions(
    DETAIL_FIELDS,
    detailFields.map((field) => field.id),
  )
  const present = new Set(
    visibleDetailFields(
      detailFields.map((field) => field.id),
      resolved,
    ),
  )
  const visible = detailFields.filter((field) => present.has(field.id))

  return (
    <FieldPickerMenu
      options={options}
      onToggle={toggleDetailField}
      onReset={resetDetailFields}
    >
      <div className="flex items-start gap-2">
        {/* The rows size to their content rather than growing, so the picker
            button that follows them hugs the block it configures instead of
            drifting off to sit beside the action icons. */}
        <dl className="grid min-w-0 grid-cols-[auto_minmax(0,1fr)] items-baseline gap-x-2 gap-y-0.5">
          {visible.map((field) => (
            <FieldRow
              emphasis={field.emphasis}
              key={field.id}
              label={getMessageField(field.id).label}
              showLabel={field.showLabel}
            >
              <FieldValue id={field.id} message={resolved} onSearch={onSearch} />
            </FieldRow>
          ))}
          {/* The arrival time is the pane's own note rather than a message
              field, so it is not in the registry and cannot be turned off or
              moved. It renders last so the fields — the subject first among
              them — stay the top of the header. */}
          <FieldRow emphasis="body" label="Arrived at" showLabel>
            <span className="font-mono">
              {formatAbsoluteDate(message.receivedAt)}
            </span>
            {threadMessageCount > 1 && (
              <span className="ml-2 font-mono text-ui text-muted-foreground/80">
                {threadMessageCount} messages
              </span>
            )}
          </FieldRow>
        </dl>
        <FieldPickerButton
          className="shrink-0 text-muted-foreground"
          label="Choose header rows"
          options={options}
          onToggle={toggleDetailField}
          onReset={resetDetailFields}
        />
      </div>
    </FieldPickerMenu>
  )
}

/**
 * A row's value. Most fields are the registry's text; two are not, and they
 * are the two the header used to draw by hand — the sender and the tags.
 * Per-field rendering is the SURFACE's business (the list does exactly this in
 * `columns.tsx`, and the registry's own doc says layout is not its concern),
 * so the richer renderings live here and the registry keeps one text form per
 * field for everyone else.
 *
 * Both are also clickable, and both search — for the sender, for the tag.
 * Those clicks came with the elements when they moved into the rows; losing
 * them would have been a feature deleted in the middle of a layout change.
 */
function FieldValue({
  id,
  message,
  onSearch,
}: {
  id: MessageFieldId
  message: MessageSummary
  onSearch?: (query: string, append?: boolean) => void
}) {
  switch (id) {
    case 'from':
      return <SenderValue message={message} onSearch={onSearch} />
    case 'tags':
      return <TagsValue message={message} onSearch={onSearch} />
    default:
      return messageFieldText(id, message)
  }
}

/** The sender: two values in one, a display name and the address behind it,
 *  either of which searches for mail from them (shift-click appends to the
 *  current query, as everywhere else). */
function SenderValue({
  message,
  onSearch,
}: {
  message: MessageSummary
  onSearch?: (query: string, append?: boolean) => void
}) {
  const senderName = messageFieldText('from', message)
  const senderEmail = message.fromEmail ?? ''

  return (
    <span className="inline-flex min-w-0 flex-wrap items-center gap-1.5">
      <button
        className="truncate font-medium hover:text-primary hover:underline"
        onClick={(event) =>
          onSearch?.(`from:${senderEmail || senderName}`, event.shiftKey)
        }
        title="Search emails from this sender"
        type="button"
      >
        {senderName}
      </button>
      {senderEmail && senderName !== senderEmail && (
        <button
          className="font-mono text-ui text-muted-foreground hover:text-primary hover:underline"
          onClick={(event) => onSearch?.(`from:${senderEmail}`, event.shiftKey)}
          title="Search emails from this sender"
          type="button"
        >
          &lt;{senderEmail}&gt;
        </button>
      )}
    </span>
  )
}

/** The message's user tags as chips, each searching for that tag. Only user
 *  tags: `userTags` drops the `$`-prefixed system keywords, which are state
 *  (read, flagged) that the message already shows in other ways. */
function TagsValue({
  message,
  onSearch,
}: {
  message: MessageSummary
  onSearch?: (query: string, append?: boolean) => void
}) {
  return (
    <span className="flex flex-wrap items-center gap-1.5">
      {userTags(message.keywords).map((tag) => (
        <Badge
          className="cursor-pointer rounded-[4px] border-border/80 bg-background/45 px-1.5 py-0.5 font-mono text-ui uppercase text-muted-foreground hover:border-primary hover:text-primary"
          key={tag}
          onClick={(event: MouseEvent) =>
            onSearch?.(`tag:${tag}`, event.shiftKey)
          }
          title={`Search emails tagged "${tag}"`}
          variant="outline"
        >
          {tag}
        </Badge>
      ))}
    </span>
  )
}

/**
 * One row. A row without its key spans both grid columns rather than leaving a
 * gap where the label would have been, so a label-less subject starts at the
 * left edge like the heading it replaced.
 *
 * The key itself stays at one size whatever the value's emphasis: it is
 * scaffolding, and a 17px `Subject:` would compete with the subject.
 */
function FieldRow({
  children,
  emphasis,
  label,
  showLabel,
}: {
  children: React.ReactNode
  emphasis: MessageFieldEmphasis
  label: string
  showLabel: boolean
}) {
  return (
    <>
      {showLabel && (
        <dt className="whitespace-nowrap text-ui text-muted-foreground/60">
          {label}:
        </dt>
      )}
      <dd
        className={cn(
          'min-w-0 break-words',
          EMPHASIS_CLASS[emphasis],
          !showLabel && 'col-span-2',
        )}
      >
        {children}
      </dd>
    </>
  )
}
