// Shell chrome that reflects facade state: the degraded-connection banner and
// the undo-send toast. The toast's countdown is local time-keeping; what it
// says about the send comes from the pending-operations query, and the
// operation-settled prompt only nudges it to re-evaluate — payloads are never
// rendered.

import { useEffect, useState } from 'react'
import { useConnectionStatus, useMailClient, usePendingOperations } from '../hooks'
import type { SentInfo } from './Compose'

export function ConnectionBanner() {
  const status = useConnectionStatus()
  if (status === 'connected') return null
  return (
    <div className="connection-banner" data-status={status} role="status">
      {status === 'reconnecting'
        ? 'Connection lost — reconnecting…'
        : 'Backend unreachable — showing the last known state'}
    </div>
  )
}

type Phase = 'holding' | 'sending' | 'sent' | 'failed'

export function UndoToast({
  sent,
  onUndo,
  onDismiss,
}: {
  sent: SentInfo
  onUndo: () => void
  onDismiss: () => void
}) {
  const client = useMailClient()
  const pending = usePendingOperations(sent.accountId)
  const [now, setNow] = useState(() => Date.now())

  useEffect(() => {
    const timer = setInterval(() => setNow(Date.now()), 500)
    return () => clearInterval(timer)
  }, [])

  // The settled prompt triggers a refetch in the facade; ticking `now` makes
  // sure this toast re-reads the refreshed pending-operations answer promptly.
  useEffect(
    () => client.onEvent('operation.settled', () => setNow(Date.now())),
    [client],
  )

  const remaining = Math.max(0, Math.ceil((sent.expiresAt - now) / 1000))
  // Exactly this send's operation: its outbox operation id is the id the
  // send verb returned, so another send's verdict can never leak in here.
  const sendOp = (pending.data?.rows ?? []).find((op) => op.id === sent.operationId) ?? null
  const failedOp = sendOp?.state === 'failed' ? sendOp : null
  const inProgress =
    sendOp !== null &&
    (sendOp.state === 'pending' ||
      sendOp.state === 'inflight' ||
      sendOp.state === 'dispatchUncertain')

  let phase: Phase
  if (failedOp) phase = 'failed'
  else if (remaining > 0) phase = 'holding'
  else if (inProgress || pending.data === undefined) phase = 'sending'
  else phase = 'sent'

  // Once the send is through, linger briefly and go away on its own.
  useEffect(() => {
    if (phase !== 'sent') return
    const timer = setTimeout(onDismiss, 2500)
    return () => clearTimeout(timer)
  }, [phase, onDismiss])

  return (
    <div className="undo-toast" data-phase={phase} role="status">
      {phase === 'holding' && (
        <>
          <span>Sending in {remaining}s</span>
          <button type="button" className="undo-btn" onClick={onUndo}>
            Undo
          </button>
        </>
      )}
      {phase === 'sending' && <span>Sending…</span>}
      {phase === 'sent' && <span>Sent</span>}
      {phase === 'failed' && (
        <>
          <span>Send failed{failedOp?.lastError ? `: ${failedOp.lastError}` : ''}</span>
          <button type="button" className="undo-btn" onClick={onDismiss}>
            Dismiss
          </button>
        </>
      )}
    </div>
  )
}
