// The reading pane: the selected message's thread, each message expandable,
// with the expanded ones fetching their full detail (sanitized bodies plus
// attachment metadata; attachment bytes come from authenticated blob GETs).
// Opening an unread message marks it read through the ordinary verb — the
// unread dot clears because the refetched answer says so.
//
// The pane is not keyboard-focusable: it renders the list's selection, and
// j/k in the list drive what it shows.

import { useEffect, useMemo, useRef, useState } from 'react'
import { useMailClient, useMessage, useThread } from '../hooks'
import type { AccountId, MessageDetailResult, MessageId, MessageSummary } from '../gen'
import { formatFullDate, formatSize, recipientLine, senderLabel } from './format'
import { sanitizeMessageHtml } from './sanitize'
import { ArchiveIcon, CloseIcon, ReplyIcon, TrashIcon } from './icons'
import type { RowVerbs } from './MailList'
import type { Selection } from './model'

export function ReadingPane({
  selection,
  verbs,
  onReply,
  onClose,
}: {
  selection: Selection
  verbs: RowVerbs
  onReply: (row: MessageSummary) => void
  onClose: () => void
}) {
  const thread = useThread({ accountId: selection.accountId, threadId: selection.threadId })
  const messages = thread.data?.messages ?? []
  const selectedSummary = messages.find((m) => m.id === selection.messageId) ?? null

  // Which thread messages are expanded; the selected one always is.
  const [expanded, setExpanded] = useState<Set<string>>(() => new Set([selection.messageId]))
  useEffect(() => {
    setExpanded((prev) => {
      if (prev.has(selection.messageId)) return prev
      const next = new Set(prev)
      next.add(selection.messageId)
      return next
    })
  }, [selection.messageId])

  const toggle = (id: MessageId) => {
    setExpanded((prev) => {
      const next = new Set(prev)
      if (next.has(id)) next.delete(id)
      else next.add(id)
      return next
    })
  }

  return (
    <section className="detail-pane" aria-label="Message">
      <header className="detail-header">
        <h2>{selectedSummary?.subject || '(no subject)'}</h2>
        <div className="detail-actions">
          {selectedSummary && (
            <>
              <button
                type="button"
                title="Reply (r)"
                onClick={() => onReply(selectedSummary)}
              >
                <ReplyIcon />
              </button>
              <button
                type="button"
                title={selectedSummary.isFlagged ? 'Unflag' : 'Flag'}
                data-flagged={selectedSummary.isFlagged || undefined}
                className="flag-btn"
                onClick={() => verbs.toggleFlag(selectedSummary)}
              >
                {selectedSummary.isFlagged ? '★' : '☆'}
              </button>
              <button
                type="button"
                title={selectedSummary.isRead ? 'Mark unread' : 'Mark read'}
                onClick={() => verbs.toggleRead(selectedSummary)}
              >
                {selectedSummary.isRead ? '◌' : '●'}
              </button>
              <button type="button" title="Archive (e)" onClick={() => verbs.archive(selectedSummary)}>
                <ArchiveIcon />
              </button>
              <button type="button" title="Trash (#)" onClick={() => verbs.trash(selectedSummary)}>
                <TrashIcon />
              </button>
            </>
          )}
          <button type="button" title="Close (Esc)" onClick={onClose}>
            <CloseIcon />
          </button>
        </div>
      </header>

      <div className="detail-scroll">
        {thread.status === 'loading' && <div className="list-note">Loading…</div>}
        {thread.status === 'error' && (
          <div className="list-note error">
            Couldn&apos;t load this thread{thread.error ? `: ${thread.error.message}` : ''}
          </div>
        )}
        {messages.map((m) => (
          <ThreadMessage
            key={m.id}
            accountId={selection.accountId}
            summary={m}
            expanded={expanded.has(m.id)}
            onToggle={() => toggle(m.id)}
          />
        ))}
      </div>
    </section>
  )
}

function ThreadMessage({
  accountId,
  summary,
  expanded,
  onToggle,
}: {
  accountId: AccountId
  summary: MessageSummary
  expanded: boolean
  onToggle: () => void
}) {
  return (
    <article className="thread-message" data-expanded={expanded || undefined}>
      <button type="button" className="thread-message-header" onClick={onToggle}>
        <span className="thread-from" data-unread={!summary.isRead || undefined}>
          {senderLabel(summary)}
        </span>
        {!expanded && <span className="thread-snippet">{summary.preview ?? ''}</span>}
        <span className="thread-date">{formatFullDate(summary.receivedAt)}</span>
      </button>
      {expanded && (
        <div className="thread-message-open">
          {summary.to.length > 0 && (
            <div className="thread-recipients">to {recipientLine(summary.to)}</div>
          )}
          <MessageBody accountId={accountId} messageId={summary.id} />
        </div>
      )}
    </article>
  )
}

function MessageBody({ accountId, messageId }: { accountId: AccountId; messageId: MessageId }) {
  const client = useMailClient()
  const detail = useMessage({ accountId, messageId })

  // Opening an unread message marks it read — once per mounted message; the
  // refetched answer flips `isRead`, so the effect does not re-fire.
  const marked = useRef(false)
  useEffect(() => {
    const summary = detail.data?.summary
    if (summary && !summary.isRead && !marked.current) {
      marked.current = true
      client.markRead(accountId, messageId).catch(() => {
        marked.current = false
      })
    }
  }, [client, accountId, messageId, detail.data])

  if (detail.status === 'loading') return <div className="list-note">Loading message…</div>
  if (detail.status === 'error' || !detail.data) {
    return (
      <div className="list-note error">
        Couldn&apos;t load this message{detail.error ? `: ${detail.error.message}` : ''}
      </div>
    )
  }

  const { bodyHtml, bodyText, attachments } = detail.data
  return (
    <div className="message-body">
      {bodyHtml ? (
        <HtmlBody html={bodyHtml} />
      ) : (
        <pre className="body-text">{bodyText ?? '(empty message)'}</pre>
      )}
      {attachments.filter((a) => !a.isInline).length > 0 && (
        <AttachmentList detail={detail.data} blobUrl={(id) => client.blobUrl(id)} />
      )}
    </div>
  )
}

/** Untrusted HTML rendered behind three independent barriers: the markup is
 * sanitized (active elements, event handlers, and script-scheme URLs are
 * stripped), the frame's CSP blocks script/plugin/form content even if
 * something slipped the sanitizer, and the iframe sandbox withholds script
 * execution. Links open in a new tab; the frame grows to its content on
 * load. */
function HtmlBody({ html }: { html: string }) {
  const safeHtml = useMemo(() => sanitizeMessageHtml(html), [html])
  const doc = `<!doctype html><html><head><meta charset="utf-8"><meta http-equiv="Content-Security-Policy" content="default-src 'none'; img-src http: https: data: cid:; style-src 'unsafe-inline' http: https:; font-src http: https: data:; form-action 'none';"><base target="_blank"><style>
    body { margin: 0; padding: 2px; font: 14px/1.5 system-ui, sans-serif; color: #1c1c1e; overflow-wrap: break-word; }
    img { max-width: 100%; height: auto; }
    a { color: #2563eb; }
  </style></head><body>${safeHtml}</body></html>`
  return (
    <iframe
      className="body-frame"
      title="Message body"
      sandbox="allow-same-origin allow-popups"
      srcDoc={doc}
      onLoad={(e) => {
        const frame = e.currentTarget
        const inner = frame.contentDocument
        if (inner) frame.style.height = `${inner.documentElement.scrollHeight + 8}px`
      }}
    />
  )
}

function AttachmentList({
  detail,
  blobUrl,
}: {
  detail: MessageDetailResult
  blobUrl: (blobId: string) => string
}) {
  return (
    <div className="attachment-list">
      {detail.attachments
        .filter((a) => !a.isInline)
        .map((a) => (
          <a
            key={a.id}
            className="attachment-chip"
            href={blobUrl(a.blobId)}
            download={a.filename ?? undefined}
            title={a.mimeType}
          >
            <span className="attachment-name">{a.filename ?? 'attachment'}</span>
            <span className="attachment-size">{formatSize(a.size)}</span>
          </a>
        ))}
    </div>
  )
}
