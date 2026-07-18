import type { SendMessageInput } from '@/data/transport/api'
import { parseEmailAddress } from '@/domain/address'

import { parseRecipients, type ComposeForm } from './model'

export function validateComposeSubmission(
  formData: ComposeForm,
  input: SendMessageInput,
): string | null {
  if (
    formData.from.trim().length > 0 &&
    parseRecipients(formData.from).length !== 1
  ) {
    return 'From address must be a single email address.'
  }
  if (!input.from || input.from.email.trim().length === 0) {
    return 'Add a From address.'
  }
  if (input.to.length === 0) {
    return 'Add at least one recipient.'
  }
  if (!parseEmailAddress(input.from.email)) {
    return 'From address must be a single email address.'
  }
  if (input.to.some((recipient) => recipient.email.trim().length === 0)) {
    return 'Recipient email addresses cannot be empty.'
  }
  if (input.subject.length === 0) {
    return 'Add a subject.'
  }
  if (input.body.trim().length === 0) {
    return 'Write a message body.'
  }
  return null
}
