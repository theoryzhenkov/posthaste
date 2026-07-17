// Small presentation helpers shared by the shell components.

import type { MessageSummary, Recipient } from '../gen'

/** Compact list-row date: time for today, month + day for this year, and a
 * full date beyond that. */
export function formatListDate(iso: string): string {
  const date = new Date(iso)
  if (Number.isNaN(date.getTime())) return ''
  const now = new Date()
  const sameDay =
    date.getFullYear() === now.getFullYear() &&
    date.getMonth() === now.getMonth() &&
    date.getDate() === now.getDate()
  if (sameDay) {
    return date.toLocaleTimeString(undefined, { hour: 'numeric', minute: '2-digit' })
  }
  if (date.getFullYear() === now.getFullYear()) {
    return date.toLocaleDateString(undefined, { month: 'short', day: 'numeric' })
  }
  return date.toLocaleDateString(undefined, { year: 'numeric', month: 'short', day: 'numeric' })
}

/** Full date + time, for the reading pane. */
export function formatFullDate(iso: string): string {
  const date = new Date(iso)
  if (Number.isNaN(date.getTime())) return iso
  return date.toLocaleString(undefined, {
    weekday: 'short',
    year: 'numeric',
    month: 'short',
    day: 'numeric',
    hour: 'numeric',
    minute: '2-digit',
  })
}

export function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(0)} KB`
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
}

/** Display name for a message's sender: name, else address, else a dash. */
export function senderLabel(m: MessageSummary): string {
  return m.fromName || m.fromEmail || '(unknown sender)'
}

export function recipientLabel(r: Recipient): string {
  return r.name ? `${r.name} <${r.email}>` : r.email
}

export function recipientLine(list: Recipient[]): string {
  return list.map(recipientLabel).join(', ')
}
