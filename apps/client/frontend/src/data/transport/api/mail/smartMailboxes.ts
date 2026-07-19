
// The smart-mailbox projection the backend answers with is the generated
// `SmartMailboxRow` twin (configuration incl. rule + live counts),
// re-exported under both historical names so the whole tree shares one type
// identity.

export type { SmartMailboxRow as SmartMailbox } from '@/gen'
export type { SmartMailboxRow as SmartMailboxSummary } from '@/gen'
