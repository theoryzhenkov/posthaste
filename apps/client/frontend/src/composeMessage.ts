import type {
  Recipient,
  SendMessageAttachmentInput,
  SendMessageInput,
} from './api/types'

export const MAX_COMPOSE_ATTACHMENTS = 10
export const MAX_COMPOSE_ATTACHMENT_BYTES = 10 * 1024 * 1024
export const MAX_COMPOSE_TOTAL_ATTACHMENT_BYTES = 25 * 1024 * 1024

export interface ComposeAttachment {
  id: string
  file: File
  filename: string
  mimeType: string
  size: number
}

export interface ComposeForm {
  from: string
  to: string
  cc: string
  bcc: string
  subject: string
  body: string
  attachments: ComposeAttachment[]
}

export const EMPTY_COMPOSE_FORM: ComposeForm = {
  from: '',
  to: '',
  cc: '',
  bcc: '',
  subject: '',
  body: '',
  attachments: [],
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

export function buildSendInput(
  form: ComposeForm,
  attachments: SendMessageAttachmentInput[] = [],
): SendMessageInput {
  return {
    from: parseSender(form.from),
    to: parseRecipients(form.to),
    cc: parseRecipients(form.cc),
    bcc: parseRecipients(form.bcc),
    subject: form.subject.trim(),
    body: form.body,
    inReplyTo: null,
    references: null,
    attachments,
  }
}

export function composeAttachmentFromFile(file: File): ComposeAttachment {
  const id = `${file.name}:${file.size}:${file.lastModified}:${crypto.randomUUID()}`
  return {
    id,
    file,
    filename: file.name || 'attachment',
    mimeType: file.type || 'application/octet-stream',
    size: file.size,
  }
}

export async function readAttachmentForSend(
  attachment: ComposeAttachment,
): Promise<SendMessageAttachmentInput> {
  const buffer = await attachment.file.arrayBuffer()
  return {
    filename: attachment.filename,
    mimeType: attachment.mimeType,
    contentBase64: bytesToBase64(new Uint8Array(buffer)),
  }
}

function bytesToBase64(bytes: Uint8Array): string {
  let binary = ''
  for (let offset = 0; offset < bytes.length; offset += 0x8000) {
    binary += String.fromCharCode(...bytes.subarray(offset, offset + 0x8000))
  }
  return btoa(binary)
}
