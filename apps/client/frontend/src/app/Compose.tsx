// The compose overlay. Everything typed here is ephemeral component state
// until submission: Send posts one send command (with the default 10-second
// undo window), Save stores a draft, and closing a dirty compose saves a
// draft implicitly. Attachments are read into the request as base64 parts.
//
// The compose mints its draft id up front and stamps it on both draft saves
// and the send request, so the send stays undoable by that stable key.

import { useMemo, useRef, useState } from 'react'
import { newId } from '../client'
import { useMailClient } from '../hooks'
import type { AccountId, AccountRow, Recipient, SendMessageRequest, SendMessageAttachment } from '../gen'
import { formatSize } from './format'
import { CloseIcon, PaperclipIcon } from './icons'

/** Undo window stamped on every send, in seconds. */
export const UNDO_WINDOW_SECONDS = 10

/** What the shell needs to run the undo toast after the compose closes. */
export interface SentInfo {
  accountId: AccountId
  draftId: string
  /** The send's outbox operation id (the command's idempotency id), so the
   * toast can watch exactly this send in the pending-operations answer. */
  operationId: string
  request: SendMessageRequest
  expiresAt: number
}

/** Prefill for a fresh compose: a reply carries recipients and threading; an
 * undone send restores its whole buffer, attachments included. */
export interface ComposeSeed {
  accountId: AccountId | null
  to: string
  cc: string
  subject: string
  body: string
  inReplyTo: string | null
  references: string | null
  attachments?: SendMessageAttachment[]
}

export function emptySeed(): ComposeSeed {
  return {
    accountId: null,
    to: '',
    cc: '',
    subject: '',
    body: '',
    inReplyTo: null,
    references: null,
  }
}

/** Parses "Name <a@b>, c@d; e@f" into recipients; bare words must contain @. */
export function parseRecipients(text: string): Recipient[] {
  const out: Recipient[] = []
  for (const part of text.split(/[,;]/)) {
    const raw = part.trim()
    if (!raw) continue
    const match = raw.match(/^(.*)<([^<>@\s]+@[^<>\s]+)>$/)
    if (match) {
      const name = match[1]!.trim().replace(/^"|"$/g, '')
      out.push({ name: name || null, email: match[2]! })
    } else if (raw.includes('@')) {
      out.push({ name: null, email: raw })
    }
  }
  return out
}

async function fileToAttachment(file: File): Promise<SendMessageAttachment> {
  const contentBase64 = await new Promise<string>((resolve, reject) => {
    const reader = new FileReader()
    reader.onerror = () => reject(reader.error ?? new Error('file read failed'))
    reader.onload = () => {
      const url = reader.result as string
      resolve(url.slice(url.indexOf(',') + 1))
    }
    reader.readAsDataURL(file)
  })
  return {
    filename: file.name,
    mimeType: file.type || 'application/octet-stream',
    contentBase64,
  }
}

export function Compose({
  accounts,
  seed,
  onClose,
  onSent,
}: {
  accounts: AccountRow[]
  seed: ComposeSeed
  onClose: () => void
  onSent: (info: SentInfo) => void
}) {
  const client = useMailClient()
  const draftId = useMemo(() => newId(), [])

  const defaultAccount =
    seed.accountId ?? accounts.find((a) => a.isDefault)?.id ?? accounts[0]?.id ?? null
  const [accountId, setAccountId] = useState<AccountId | null>(defaultAccount)
  const [to, setTo] = useState(seed.to)
  const [cc, setCc] = useState(seed.cc)
  const [subject, setSubject] = useState(seed.subject)
  const [body, setBody] = useState(seed.body)
  const [attachments, setAttachments] = useState<SendMessageAttachment[]>(
    seed.attachments ?? [],
  )
  const [busy, setBusy] = useState<'send' | 'save' | null>(null)
  const [error, setError] = useState<string | null>(null)
  const fileInput = useRef<HTMLInputElement>(null)

  const dirty =
    to !== seed.to ||
    cc !== seed.cc ||
    subject !== seed.subject ||
    body !== seed.body ||
    attachments.length !== (seed.attachments?.length ?? 0)

  const buildRequest = (): SendMessageRequest => ({
    from: null,
    to: parseRecipients(to),
    cc: parseRecipients(cc),
    bcc: [],
    subject,
    body,
    inReplyTo: seed.inReplyTo,
    references: seed.references,
    attachments,
    draftId,
  })

  const addFiles = async (files: FileList | null) => {
    if (!files) return
    try {
      const added = await Promise.all(Array.from(files).map(fileToAttachment))
      setAttachments((prev) => [...prev, ...added])
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
    }
  }

  const send = async () => {
    if (!accountId) {
      setError('No account to send from')
      return
    }
    if (parseRecipients(to).length === 0) {
      setError('Add at least one recipient')
      return
    }
    setBusy('send')
    setError(null)
    try {
      const request = buildRequest()
      const { operationId } = await client.send(accountId, request, {
        undoWindowSeconds: UNDO_WINDOW_SECONDS,
      })
      onSent({
        accountId,
        draftId,
        operationId,
        request,
        expiresAt: Date.now() + UNDO_WINDOW_SECONDS * 1000,
      })
      onClose()
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
      setBusy(null)
    }
  }

  const saveDraft = async () => {
    if (!accountId) return
    setBusy('save')
    setError(null)
    try {
      await client.saveDraft(accountId, buildRequest())
      onClose()
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
      setBusy(null)
    }
  }

  const close = () => {
    // Closing a dirty compose keeps the work as a draft; failures surface as
    // pending-operations state, so the close never blocks on the network.
    if (dirty && accountId) {
      client.saveDraft(accountId, buildRequest()).catch(() => {})
    }
    onClose()
  }

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Escape') {
      e.stopPropagation()
      close()
    }
    if ((e.metaKey || e.ctrlKey) && e.key === 'Enter') {
      e.preventDefault()
      void send()
    }
  }

  return (
    <div className="compose-backdrop" onKeyDown={onKeyDown}>
      <div className="compose-panel" role="dialog" aria-label="Compose message">
        <header className="compose-header">
          <span>{seed.inReplyTo ? 'Reply' : 'New message'}</span>
          <button type="button" title="Close (Esc)" onClick={close}>
            <CloseIcon />
          </button>
        </header>

        {accounts.length > 1 && (
          <label className="compose-field">
            <span>From</span>
            <select
              value={accountId ?? ''}
              onChange={(e) => setAccountId(e.target.value || null)}
            >
              {accounts.map((a) => (
                <option key={a.id} value={a.id}>
                  {a.name}
                </option>
              ))}
            </select>
          </label>
        )}
        <label className="compose-field">
          <span>To</span>
          <input
            autoFocus
            type="text"
            value={to}
            placeholder="name@example.com, …"
            onChange={(e) => setTo(e.target.value)}
          />
        </label>
        <label className="compose-field">
          <span>Cc</span>
          <input type="text" value={cc} onChange={(e) => setCc(e.target.value)} />
        </label>
        <label className="compose-field">
          <span>Subject</span>
          <input type="text" value={subject} onChange={(e) => setSubject(e.target.value)} />
        </label>

        <textarea
          className="compose-body"
          value={body}
          onChange={(e) => setBody(e.target.value)}
        />

        {attachments.length > 0 && (
          <div className="attachment-list compose-attachments">
            {attachments.map((a, i) => (
              <span key={`${a.filename}-${i}`} className="attachment-chip">
                <span className="attachment-name">{a.filename}</span>
                <span className="attachment-size">
                  {formatSize(Math.floor((a.contentBase64.length * 3) / 4))}
                </span>
                <button
                  type="button"
                  title="Remove attachment"
                  onClick={() => setAttachments((prev) => prev.filter((_, j) => j !== i))}
                >
                  <CloseIcon />
                </button>
              </span>
            ))}
          </div>
        )}

        {error && <div className="compose-error">{error}</div>}

        <footer className="compose-footer">
          <button
            type="button"
            className="primary"
            disabled={busy !== null}
            onClick={() => void send()}
          >
            {busy === 'send' ? 'Sending…' : 'Send'}
          </button>
          <button type="button" disabled={busy !== null} onClick={() => void saveDraft()}>
            {busy === 'save' ? 'Saving…' : 'Save draft'}
          </button>
          <button
            type="button"
            title="Attach files"
            onClick={() => fileInput.current?.click()}
          >
            <PaperclipIcon />
          </button>
          <input
            ref={fileInput}
            type="file"
            multiple
            hidden
            onChange={(e) => {
              void addFiles(e.target.files)
              e.target.value = ''
            }}
          />
          <span className="compose-hint">Ctrl/⌘+Enter to send · sends hold {UNDO_WINDOW_SECONDS}s for undo</span>
        </footer>
      </div>
    </div>
  )
}
