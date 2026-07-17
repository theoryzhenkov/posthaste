// The mail shell: sidebar, list, and reading pane over the facade's live
// queries, plus the compose overlay, the undo-send toast, and the connection
// banner. One window keydown listener owns every shortcut so a key's meaning
// never depends on which component happens to be mounted; while the compose
// overlay is open it owns input and the mail map stands down.
//
// Shell state here is ephemera only — the effective view, the selection, the
// active pane, open overlays. Everything mail-shaped renders straight from
// query answers.

import { useEffect, useMemo, useRef, useState } from 'react'
import { useAccounts, useMailboxCounts, useMailClient, usePendingOperations } from '../hooks'
import type { MailListQuery, MessageSummary } from '../gen'
import { Compose, emptySeed, type ComposeSeed, type SentInfo } from './Compose'
import { recipientLine } from './format'
import { ComposeIcon } from './icons'
import { MailList, type RowVerbs } from './MailList'
import {
  selectionFor,
  viewForSidebarRow,
  viewKey,
  type Pane,
  type Selection,
  type SidebarRow,
  type View,
} from './model'
import { ReadingPane } from './ReadingPane'
import { groupMailboxRows, Sidebar } from './Sidebar'
import { ConnectionBanner, UndoToast } from './Toasts'
import { useMailListPages } from './useMailListPages'

/** Everything the window key handler needs, captured fresh each render. */
interface KeyContext {
  composeOpen: boolean
  step: (dir: 1 | -1) => void
  rotatePane: () => void
  clearSelection: () => void
  archiveSelected: () => void
  trashSelected: () => void
  replySelected: () => void
  openCompose: () => void
}

function isEditableTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false
  if (target.isContentEditable) return true
  const tag = target.tagName
  return tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT'
}

/** The keyboard map: chords first, then the typing guard, then bare-surface
 * keys — j/k step the active pane, Shift+H/L rotate it, e archives, # (or
 * Backspace) trashes, r replies, c composes, Escape closes the reader. */
function handleKey(e: KeyboardEvent, ctx: KeyContext): void {
  if (ctx.composeOpen) return // the compose surface owns input
  const mod = e.metaKey || e.ctrlKey
  const lower = e.key.toLowerCase()

  if (mod && lower === 'n') {
    e.preventDefault()
    ctx.openCompose()
    return
  }
  if (mod && lower === 'r') {
    e.preventDefault()
    ctx.replySelected()
    return
  }

  if (isEditableTarget(e.target) || mod || e.altKey) return

  if (e.shiftKey && (lower === 'h' || lower === 'l')) {
    e.preventDefault()
    ctx.rotatePane()
    return
  }

  switch (e.key) {
    case 'j':
    case 'ArrowDown':
      e.preventDefault()
      ctx.step(1)
      return
    case 'k':
    case 'ArrowUp':
      e.preventDefault()
      ctx.step(-1)
      return
    case 'Escape':
      ctx.clearSelection()
      return
    case 'e':
      ctx.archiveSelected()
      return
    case '#':
    case 'Backspace':
      e.preventDefault()
      ctx.trashSelected()
      return
    case 'r':
      ctx.replySelected()
      return
    case 'c':
      ctx.openCompose()
      return
  }
}

export function App() {
  const client = useMailClient()
  const accounts = useAccounts()
  const counts = useMailboxCounts()
  const pending = usePendingOperations()

  const [view, setView] = useState<View>({ kind: 'all' })
  const [selected, setSelected] = useState<Selection | null>(null)
  const [activePane, setActivePane] = useState<Pane>('list')
  const [compose, setCompose] = useState<ComposeSeed | null>(null)
  const [sent, setSent] = useState<SentInfo | null>(null)
  const [notice, setNotice] = useState<string | null>(null)

  const scope = useMemo<MailListQuery>(
    () =>
      view.kind === 'mailbox' ? { accountId: view.accountId, mailboxId: view.mailboxId } : {},
    [view],
  )
  const list = useMailListPages(scope)

  const accountRows = accounts.data?.rows ?? []
  const countRows = counts.data?.rows ?? []

  // The selectable sidebar rows for j/k in the sidebar, in the display order
  // the backend answered with.
  const sidebarRows = useMemo<SidebarRow[]>(() => {
    const rows: SidebarRow[] = [{ type: 'all' }]
    const grouped = groupMailboxRows(countRows)
    for (const account of accountRows) {
      for (const box of grouped.get(account.id) ?? []) {
        rows.push({ type: 'mailbox', accountId: account.id, mailbox: box.mailbox })
      }
    }
    return rows
  }, [accountRows, countRows])

  const report = (err: unknown) =>
    setNotice(err instanceof Error ? err.message : String(err))

  useEffect(() => {
    if (!notice) return
    const timer = setTimeout(() => setNotice(null), 5000)
    return () => clearTimeout(timer)
  }, [notice])

  // Removing the open message steps the selection to its neighbor first, so
  // the reader keeps a message open while the refetched list catches up.
  const advancePast = (id: string) => {
    setSelected((prev) => {
      if (!prev || prev.messageId !== id) return prev
      const idx = list.rows.findIndex((row) => row.id === id)
      const next = list.rows[idx + 1] ?? list.rows[idx - 1]
      return next && next.id !== id ? selectionFor(next) : null
    })
  }

  const verbs: RowVerbs = {
    archive: (row) => {
      advancePast(row.id)
      client.archive(row.sourceId, row.id).catch(report)
    },
    trash: (row) => {
      advancePast(row.id)
      client.trash(row.sourceId, row.id).catch(report)
    },
    toggleRead: (row) => {
      const verb = row.isRead ? client.markUnread : client.markRead
      verb.call(client, row.sourceId, row.id).catch(report)
    },
    toggleFlag: (row) => {
      const verb = row.isFlagged ? client.unflag : client.flag
      verb.call(client, row.sourceId, row.id).catch(report)
    },
  }

  const selectView = (next: View) => {
    if (viewKey(next) !== viewKey(view)) setSelected(null)
    setView(next)
  }

  const selectRow = (row: MessageSummary) => {
    setSelected(selectionFor(row))
    setActivePane('list')
  }

  const selectedRow = selected
    ? (list.rows.find((row) => row.id === selected.messageId) ?? null)
    : null

  const openCompose = () => setCompose(emptySeed())

  const openReply = (row: MessageSummary) => {
    const subject = row.subject ?? ''
    setCompose({
      accountId: row.sourceId,
      to: row.fromEmail
        ? row.fromName
          ? `${row.fromName} <${row.fromEmail}>`
          : row.fromEmail
        : '',
      cc: '',
      subject: /^re:/i.test(subject) ? subject : `Re: ${subject}`,
      body: '',
      inReplyTo: row.rfcMessageId ?? null,
      references: row.rfcMessageId ?? null,
    })
  }

  // Undo a held send: discard it by its stable draft key and put the whole
  // buffer back in front of the user.
  const undoSend = () => {
    if (!sent) return
    const { accountId, draftId, request } = sent
    setSent(null)
    client.discardDraft(accountId, draftId).catch(report)
    setCompose({
      accountId,
      to: recipientLine(request.to),
      cc: recipientLine(request.cc),
      subject: request.subject,
      body: request.body,
      inReplyTo: request.inReplyTo,
      references: request.references,
      attachments: request.attachments,
    })
  }

  const step = (dir: 1 | -1) => {
    if (activePane === 'sidebar') {
      const currentKey = viewKey(view)
      const idx = sidebarRows.findIndex(
        (row) => viewKey(viewForSidebarRow(row)) === currentKey,
      )
      const nextIdx =
        idx === -1 ? 0 : Math.min(Math.max(idx + dir, 0), sidebarRows.length - 1)
      const next = sidebarRows[nextIdx]
      if (next) selectView(viewForSidebarRow(next))
      return
    }
    if (list.rows.length === 0) return
    // With rows but no selection the list anchors to its first row, so the
    // navigation cursor is always a highlighted message.
    const idx = selected ? list.rows.findIndex((row) => row.id === selected.messageId) : -1
    const next =
      idx === -1
        ? list.rows[0]
        : list.rows[Math.min(Math.max(idx + dir, 0), list.rows.length - 1)]
    if (next) setSelected(selectionFor(next))
  }

  const rotatePane = () => {
    const next: Pane = activePane === 'sidebar' ? 'list' : 'sidebar'
    setActivePane(next)
    if (next === 'list' && !selected && list.rows.length > 0) {
      setSelected(selectionFor(list.rows[0]!))
    }
  }

  // One window listener for the whole surface; the context is re-captured on
  // every render so the handler always sees current state.
  const keyCtx = useRef<KeyContext>(null as unknown as KeyContext)
  keyCtx.current = {
    composeOpen: compose !== null,
    step,
    rotatePane,
    clearSelection: () => setSelected(null),
    archiveSelected: () => selectedRow && verbs.archive(selectedRow),
    trashSelected: () => selectedRow && verbs.trash(selectedRow),
    replySelected: () => selectedRow && openReply(selectedRow),
    openCompose,
  }
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => handleKey(e, keyCtx.current)
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [])

  return (
    <div className="app">
      <ConnectionBanner />
      <div className="app-main" data-detail-open={selected ? '' : undefined}>
        <Sidebar
          accounts={accountRows}
          mailboxes={countRows}
          pending={pending.data?.rows ?? []}
          view={view}
          activePane={activePane}
          onSelectView={selectView}
          onActivate={() => setActivePane('sidebar')}
        />
        <MailList
          list={list}
          title={view.kind === 'all' ? 'All Mail' : view.name}
          selection={selected}
          paneActive={activePane === 'list'}
          onSelect={selectRow}
          onActivate={() => setActivePane('list')}
          verbs={verbs}
        />
        {selected && (
          <ReadingPane
            selection={selected}
            verbs={verbs}
            onReply={openReply}
            onClose={() => setSelected(null)}
          />
        )}
      </div>

      <button type="button" className="compose-fab" title="Compose (c)" onClick={openCompose}>
        <ComposeIcon />
        <span>Compose</span>
      </button>

      {notice && (
        <div className="notice-toast" role="alert">
          {notice}
        </div>
      )}
      {compose && (
        <Compose
          accounts={accountRows}
          seed={compose}
          onClose={() => setCompose(null)}
          onSent={setSent}
        />
      )}
      {sent && <UndoToast sent={sent} onUndo={undoSend} onDismiss={() => setSent(null)} />}
    </div>
  )
}
