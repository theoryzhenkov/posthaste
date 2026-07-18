/**
 * Global (app-level) command definitions: the always-available palette entries
 * (settings surfaces, keyboard reference, tag/mailbox/rule/account management)
 * and the desktop devtools chord. They delegate to `services.app` /
 * `services.desktop`; no logic is duplicated.
 */
import {
  Keyboard,
  Settings,
  SlidersHorizontal,
  Tag,
  UserPlus,
  Wrench,
} from 'lucide-react'
import { registerActions } from '../registry'
import type { ActionDefinition } from '../types'

const globalActions: readonly ActionDefinition[] = [
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
    surfaces: ['palette', 'keyboard'],
    // Modifier-chord tier: ⌘, fires even while typing or above an overlay.
    shortcut: { key: ',', mod: true, inEditable: true, aboveOverlay: true },
    isAvailable: (_ctx, s) => Boolean(s.app),
    run: (_ctx, s) => s.app?.handleOpenSettings(),
  },
  {
    // Toggle (not show): `?` must also CLOSE the reference it opened — the
    // chord fires above overlays, and the open reference is one of them. From
    // the palette the reference is always closed, so toggle ≡ show there.
    id: 'app.shortcuts',
    section: 'app',
    title: 'Keyboard shortcuts',
    icon: Keyboard,
    keywords: 'keyboard shortcuts help',
    surfaces: ['palette', 'keyboard'],
    shortcut: { key: '?', aboveOverlay: true },
    isAvailable: (_ctx, s) => Boolean(s.app),
    run: (_ctx, s) => s.app?.handleToggleShortcuts(),
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
  {
    // ⌘⌥I toggles the desktop devtools — the charter's first R4 migration
    // (formerly App.tsx's own window listener). `code` chord: macOS ⌥I mangles
    // `event.key` into a dead key, so matching is physical. Availability rides
    // the lab toggle via the `desktop` scope service, bound once at the app
    // root; web/test hosts never bind it, so the chord stays inert there.
    id: 'app.toggle-devtools',
    section: 'app',
    title: 'Toggle developer tools',
    icon: Wrench,
    keywords: 'developer tools devtools inspect debug',
    surfaces: ['keyboard'],
    shortcut: { key: 'i', code: 'KeyI', mod: true, alt: true, inEditable: true },
    isAvailable: (_ctx, s) => s.desktop?.isDeveloperToolsEnabled() ?? false,
    run: (_ctx, s) => void s.desktop?.toggleDevtools(),
  },
]

registerActions(globalActions)
