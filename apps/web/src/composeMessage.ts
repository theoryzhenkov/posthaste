import type { Recipient, SendMessageInput } from './api/types'

export interface ComposeForm {
  from: string
  to: string
  cc: string
  bcc: string
  subject: string
  body: string
}

export const EMPTY_COMPOSE_FORM: ComposeForm = {
  from: '',
  to: '',
  cc: '',
  bcc: '',
  subject: '',
  body: '',
}

export function formatRecipient(recipient: Recipient): string {
  return recipient.name
    ? `${recipient.name} <${recipient.email}>`
    : recipient.email
}

export function formatRecipients(recipients: Recipient[]): string {
  return recipients.map(formatRecipient).join(', ')
}

export function parseRecipients(value: string): Recipient[] {
  return value
    .split(/[;,]/)
    .map((part) => part.trim())
    .filter(Boolean)
    .map((part) => {
      const match = part.match(/^(.*)<([^>]+)>$/)
      if (!match) {
        return { name: null, email: part }
      }
      const name = match[1].trim().replace(/^"|"$/g, '')
      return {
        name: name || null,
        email: match[2].trim(),
      }
    })
}

export function parseSender(value: string): Recipient | null {
  return parseRecipients(value)[0] ?? null
}

export function buildSendInput(form: ComposeForm): SendMessageInput {
  return {
    from: parseSender(form.from),
    to: parseRecipients(form.to),
    cc: parseRecipients(form.cc),
    bcc: parseRecipients(form.bcc),
    subject: form.subject.trim(),
    body: form.body,
    inReplyTo: null,
    references: null,
  }
}
