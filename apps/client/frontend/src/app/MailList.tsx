// The list pane: windowed message rows with selection, per-row quick actions,
// and cursor-driven infinite scroll (nearing the bottom loads the next page).
// Rows render query answers only; the component's own state is nothing but
// the scroll position the browser already keeps.

import { useEffect, useRef } from 'react'
import type { MessageSummary } from '../gen'
import { formatListDate, senderLabel } from './format'
import { ArchiveIcon, PaperclipIcon, TrashIcon } from './icons'
import type { MailListPages } from './useMailListPages'
import type { Selection } from './model'

export interface RowVerbs {
  archive: (row: MessageSummary) => void
  trash: (row: MessageSummary) => void
  toggleRead: (row: MessageSummary) => void
  toggleFlag: (row: MessageSummary) => void
}

const NEAR_BOTTOM_PX = 400

export function MailList({
  list,
  title,
  selection,
  paneActive,
  onSelect,
  onActivate,
  verbs,
}: {
  list: MailListPages
  title: string
  selection: Selection | null
  paneActive: boolean
  onSelect: (row: MessageSummary) => void
  onActivate: () => void
  verbs: RowVerbs
}) {
  const scrollRef = useRef<HTMLDivElement>(null)

  const onScroll = () => {
    const el = scrollRef.current
    if (!el || !list.hasMore || list.loadingMore) return
    if (el.scrollTop + el.clientHeight >= el.scrollHeight - NEAR_BOTTOM_PX) list.loadMore()
  }

  return (
    <section
      className="list-pane"
      data-active={paneActive || undefined}
      onPointerDown={onActivate}
      aria-label="Message list"
    >
      <header className="list-header">
        <h1>{title}</h1>
        {list.status === 'stale' && <span className="stale-tag">offline copy</span>}
      </header>
      <div className="list-scroll" ref={scrollRef} onScroll={onScroll}>
        {list.status === 'loading' && <div className="list-note">Loading…</div>}
        {list.status === 'error' && (
          <div className="list-note error">
            Couldn&apos;t load this view{list.error ? `: ${list.error.message}` : ''}
          </div>
        )}
        {list.status !== 'loading' && list.rows.length === 0 && list.status !== 'error' && (
          <div className="list-note">No messages</div>
        )}
        {list.rows.map((row) => (
          <MailRow
            key={row.id}
            row={row}
            selected={selection?.messageId === row.id}
            paneActive={paneActive}
            onSelect={onSelect}
            verbs={verbs}
          />
        ))}
        {list.loadingMore && <div className="list-note">Loading more…</div>}
      </div>
    </section>
  )
}

function MailRow({
  row,
  selected,
  paneActive,
  onSelect,
  verbs,
}: {
  row: MessageSummary
  selected: boolean
  paneActive: boolean
  onSelect: (row: MessageSummary) => void
  verbs: RowVerbs
}) {
  const ref = useRef<HTMLDivElement>(null)

  // Keep the selected row visible as j/k step the selection.
  useEffect(() => {
    if (selected) ref.current?.scrollIntoView({ block: 'nearest' })
  }, [selected])

  const stop = (e: React.SyntheticEvent, run: () => void) => {
    e.stopPropagation()
    run()
  }

  return (
    <div
      ref={ref}
      className="mail-row"
      data-selected={selected || undefined}
      data-pane-active={paneActive || undefined}
      data-unread={!row.isRead || undefined}
      onClick={() => onSelect(row)}
      role="button"
      tabIndex={-1}
    >
      <span className="row-unread-dot" aria-label={row.isRead ? undefined : 'Unread'} />
      <button
        type="button"
        className="row-flag"
        data-flagged={row.isFlagged || undefined}
        title={row.isFlagged ? 'Unflag' : 'Flag'}
        onClick={(e) => stop(e, () => verbs.toggleFlag(row))}
      >
        {row.isFlagged ? '★' : '☆'}
      </button>
      <div className="row-main">
        <div className="row-top">
          <span className="row-sender">{senderLabel(row)}</span>
          {row.hasAttachment && (
            <span className="row-clip" title="Has attachment">
              <PaperclipIcon />
            </span>
          )}
          <span className="row-date">{formatListDate(row.receivedAt)}</span>
        </div>
        <div className="row-bottom">
          <span className="row-subject">{row.subject || '(no subject)'}</span>
          {row.preview && <span className="row-preview"> — {row.preview}</span>}
        </div>
      </div>
      <div className="row-actions">
        <button
          type="button"
          title={row.isRead ? 'Mark unread' : 'Mark read'}
          onClick={(e) => stop(e, () => verbs.toggleRead(row))}
        >
          {row.isRead ? '◌' : '●'}
        </button>
        <button type="button" title="Archive (e)" onClick={(e) => stop(e, () => verbs.archive(row))}>
          <ArchiveIcon />
        </button>
        <button type="button" title="Trash (#)" onClick={(e) => stop(e, () => verbs.trash(row))}>
          <TrashIcon />
        </button>
      </div>
    </div>
  )
}
