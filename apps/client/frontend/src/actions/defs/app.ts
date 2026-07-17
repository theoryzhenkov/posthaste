/**
 * App-level command definitions.
 *
 * The palette-only global commands (compose, settings surfaces, keyboard
 * reference, tag/mailbox/rule/account management) expressed as
 * {@link ActionDefinition}s so the registry-backed palette provider is their
 * single source of truth. They delegate to the existing `services.app` (the
 * `useMailClientHandlers` bundle); no logic is duplicated.
 *
 * These are always available (no selection needed) and carry no `isAvailable`
 * gate. Message-scoped palette commands (reply/archive/flag/tag/snooze/…) live
 * in `defs/message.ts`.
 */
import {
  Keyboard,
  PenSquare,
  Settings,
  SlidersHorizontal,
  Tag,
  UserPlus,
} from 'lucide-react'
import { registerActions } from '../registry'
import type { ActionDefinition } from '../types'

export const appActions: readonly ActionDefinition[] = [
  {
    id: 'app.compose',
    section: 'compose-reply',
    title: 'Compose new message',
    icon: PenSquare,
    keywords: 'compose new message draft',
    surfaces: ['palette'],
    shortcut: { key: 'n', mod: true },
    run: (_ctx, s) => s.app?.handleCompose(),
  },
  {
    id: 'app.new-smart-mailbox',
    section: 'app',
    title: 'New smart mailbox…',
    icon: SlidersHorizontal,
    keywords: 'new smart mailbox create filter',
    surfaces: ['palette'],
    run: (_ctx, s) => s.app?.handleOpenSettings('mailboxes'),
  },
  {
    id: 'app.new-rule',
    section: 'app',
    title: 'New rule for mailbox…',
    icon: SlidersHorizontal,
    keywords: 'rule mailbox saved search',
    surfaces: ['palette'],
    run: (_ctx, s) => s.app?.handleOpenSettings('mailboxes'),
  },
  {
    id: 'app.manage-tags',
    section: 'app',
    title: 'Manage tags',
    icon: Tag,
    keywords: 'manage tags rename delete labels',
    surfaces: ['palette'],
    run: (_ctx, s) => s.app?.handleOpenSettings('tags'),
  },
  {
    id: 'app.open-settings',
    section: 'app',
    title: 'Open Settings',
    icon: Settings,
    keywords: 'settings preferences',
    surfaces: ['palette'],
    shortcut: { key: ',', mod: true },
    run: (_ctx, s) => s.app?.handleOpenSettings(),
  },
  {
    id: 'app.shortcuts',
    section: 'app',
    title: 'Keyboard shortcuts',
    icon: Keyboard,
    keywords: 'keyboard shortcuts help',
    surfaces: ['palette'],
    shortcut: { key: '?' },
    run: (_ctx, s) => s.app?.handleShowShortcuts(),
  },
  {
    id: 'app.add-account',
    section: 'app',
    title: 'Add account…',
    icon: UserPlus,
    keywords: 'account add source login',
    surfaces: ['palette'],
    run: (_ctx, s) => s.app?.handleOpenSettings('accounts'),
  },
]

registerActions(appActions)
