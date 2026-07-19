import type { ReactNode } from 'react'

import type { ActionParamOption } from '@/commands'
import type { ComposeIntent } from '@/domain/composeIntent'
import type { SettingsSurfaceCategory } from '@/domain/surface'

export type SearchVertical =
  | 'command'
  | 'query-completion'
  | 'mailbox'
  | 'tag'
  | 'contact'
  | 'message'

export interface SearchProvider {
  id: string
  label: string
  vertical: SearchVertical
  /** Issues a backend request, so its dispatch is debounced while typing. */
  remote?: boolean
  search(req: ProviderSearchRequest): Promise<ProviderResultPage>
}

export interface ProviderSearchRequest {
  query: string
  cursor?: string | null
  limit: number
  context: RankingContext
  signal?: AbortSignal
}

interface ProviderResultPage {
  candidates: SearchCandidate[]
  nextCursor: string | null
  indexVersion?: string
  latencyMs?: number
}

export interface SearchCandidate {
  id: string
  providerId: string
  vertical: SearchVertical
  entry: CommandPaletteEntry
  providerRank: number
  providerScore?: number
  match: MatchEvidence
  features: SearchFeatureMap
}

type SearchFeatureMap = Record<string, number | boolean | string>

export interface MatchEvidence {
  query: string
  fields: Array<{
    field:
      | 'label'
      | 'subtitle'
      | 'keywords'
      | 'from'
      | 'subject'
      | 'body'
      | 'mailbox'
      | 'tag'
    kind: 'exact' | 'prefix' | 'acronym' | 'fuzzy' | 'contains' | 'fts'
    ranges?: Array<{ start: number; end: number }>
  }>
}

export interface CommandPaletteEntry {
  id: string
  kind: SearchVertical
  label: string
  subtitle?: string
  keywords: string
  icon?: ReactNode
  action: PaletteAction
  closeOnSelect?: boolean
  /** Registry-backed palette rows: shown greyed and non-runnable when the
   *  action's `isEnabled` predicate fails. */
  disabled?: boolean
  /** Hint rendered under a disabled row (e.g. "Select a message first"). */
  disabledReason?: string
  /** Pre-formatted shortcut hint (e.g. `⌘⇧L`) rendered right-aligned on the
   *  row. */
  shortcut?: string
}

export type PaletteAction =
  | { kind: 'command'; commandId: CommandActionId }
  /** Registry action: dispatched via `getAction(actionId).run(ctx, services)`
   *  — the palette's single execution path. */
  | { kind: 'action'; actionId: string }
  /** PARAMETERIZED registry action: selecting it pushes the palette into its
   *  pick-step (a searchable list of the action's options) instead of running. */
  | { kind: 'open-action-params'; actionId: string }
  /** One picked option of a parameterized action (a pick-step row): runs
   *  `getAction(actionId).run(ctx, services, param)`. */
  | { kind: 'run-action-param'; actionId: string; param: ActionParamOption }
  | { kind: 'apply-query'; query: string }
  | {
      kind: 'open-source-mailbox'
      sourceId: string
      mailboxId: string
      name: string
    }
  | { kind: 'open-smart-mailbox'; smartMailboxId: string; name: string }
  | {
      kind: 'open-message'
      sourceId: string
      messageId: string
      conversationId: string
      mailboxHint?: { mailboxId: string; name: string }
    }
  | { kind: 'open-settings'; category?: SettingsSurfaceCategory }
  | { kind: 'open-compose'; intent: ComposeIntent }
  | { kind: 'open-contact'; contactId: string; query: string }
  | { kind: 'replace-query'; query: string }
  | { kind: 'add-tag-to-message'; tag: string }
  | { kind: 'remove-tag-from-message'; tag: string }
  | { kind: 'open-tag-editor' }
  | { kind: 'noop'; label: string }

type CommandActionId =
  | 'compose'
  | 'reply'
  | 'archive'
  | 'flag'
  | 'snooze'
  | 'newSmart'
  | 'newRule'
  | 'settings'
  | 'shortcuts'
  | 'account'

export interface DecayedCounter {
  halfLifeMs: number
  entries: Record<string, { value: number; updatedAt: number }>
}

interface LocalRankingModelSnapshot {
  version: string
  featureWeights: Record<string, number>
}

export interface RankingContext {
  now: number
  app: {
    route: 'inbox' | 'mailbox' | 'thread' | 'search' | 'composer' | 'settings'
    accountId?: string
    mailboxId?: string
    selectedMessageId?: string
    selectedThreadId?: string
    selectedContactId?: string
    composerState?: 'none' | 'new' | 'reply' | 'forward'
    hasSelectedMessage: boolean
  }
  session: {
    paletteOpenReason: 'keyboard' | 'button' | 'command-chain'
    previousPaletteQuery?: string
    lastActionId?: string
  }
  user: {
    recentCommands: DecayedCounter
    recentEntities: DecayedCounter
    frequentCommands: DecayedCounter
    frequentMailboxes: DecayedCounter
    frequentContacts: DecayedCounter
    pinnedCommands: string[]
    pinnedMailboxes: string[]
  }
  model?: LocalRankingModelSnapshot
}

export interface ProviderState {
  status: 'idle' | 'loading' | 'done' | 'error'
  candidates: SearchCandidate[]
  nextCursor: string | null
  error?: unknown
  latencyMs?: number
  indexVersion?: string
}

export type PaletteRow =
  | { kind: 'section'; id: string; label: string }
  | { kind: 'item'; id: string; candidate: SearchCandidate }
  | { kind: 'loading'; id: string; providerId: string; label: string }
  | { kind: 'empty'; id: string; providerId: string; label: string }
  | {
      kind: 'error'
      id: string
      providerId: string
      label: string
      message: string
    }

export interface CommandSearchSession {
  query: string
  queryVersion: number
  context: RankingContext
  providerStates: Map<string, ProviderState>
  rows: PaletteRow[]
  selectedCandidateId: string | null
  isLoading: boolean
  isSettled: boolean
  cancelledSearchCount: number
  staleSearchCount: number
}

export interface CommandSearchController {
  session: CommandSearchSession
  loadMore(providerId: string): void
  cancel(): void
  select(candidateId: string | null): void
}
