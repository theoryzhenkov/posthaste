import {
  ArrowLeft,
  Bell,
  FolderSearch,
  HardDrive,
  Mailbox,
  Palette,
  Settings as SettingsIcon,
  Wrench,
} from 'lucide-react'

import { brandAccents } from '../../design/tokens'
import { isTauriRuntime } from '../../desktop'
import { cn } from '../../lib/utils'
import { isNightly, releaseChannel } from '../../runtime/releaseChannel'
import type { SettingsSurfaceCategory } from '../../surfaces'
import { Button } from '../ui/button'

export type SettingsCategory = SettingsSurfaceCategory

const SETTINGS_CATEGORIES = [
  {
    id: 'general',
    label: 'General',
    description: 'Default account and workspace-wide preferences.',
    icon: SettingsIcon,
    accent: brandAccents.blue,
  },
  {
    id: 'appearance',
    label: 'Appearance',
    description: 'Built-in themes, color mode, and density.',
    icon: Palette,
    accent: brandAccents.sage,
  },
  {
    id: 'accounts',
    label: 'Accounts',
    description: 'Connected mail sources, sync state, and credentials.',
    icon: Mailbox,
    accent: brandAccents.coral,
  },
  {
    id: 'storage',
    label: 'Storage',
    description: 'Cache size and what to keep on this device.',
    icon: HardDrive,
    accent: brandAccents.amber,
  },
  {
    id: 'notifications',
    label: 'Notifications',
    description: 'New-mail alerts and sounds.',
    icon: Bell,
    accent: brandAccents.rose,
  },
  {
    id: 'mailboxes',
    label: 'Mailboxes & Rules',
    description: 'Smart mailboxes and rules that shape your views.',
    icon: FolderSearch,
    accent: brandAccents.violet,
  },
  {
    id: 'troubleshooting',
    label: 'Troubleshooting',
    description: 'Repair, reset, and developer tools.',
    icon: Wrench,
    accent: brandAccents.coralDeep,
  },
] as const

export function SettingsRail({
  activeCategory,
  accountCount,
  smartMailboxCount,
  onClose,
  onSelect,
}: {
  activeCategory: SettingsCategory
  accountCount: number
  smartMailboxCount: number
  onClose?: () => void
  onSelect: (category: SettingsCategory) => void
}) {
  const categories = isTauriRuntime()
    ? SETTINGS_CATEGORIES
    : SETTINGS_CATEGORIES.filter(
        (category) => category.id !== 'troubleshooting',
      )

  return (
    <aside className="flex max-h-[190px] min-h-0 w-full shrink-0 flex-col border-b border-sidebar-border bg-sidebar text-sidebar-foreground md:h-full md:max-h-none md:w-[210px] md:border-b-0 md:border-r">
      <div className="flex h-12 shrink-0 items-center px-4 md:h-14">
        {onClose && (
          <Button
            type="button"
            variant="ghost"
            size="sm"
            onClick={onClose}
            className="h-7 rounded-[5px] px-2 text-[13px] font-medium text-sidebar-foreground/70 hover:bg-sidebar-accent hover:text-sidebar-accent-foreground"
          >
            <ArrowLeft size={15} strokeWidth={1.6} />
            Back to app
          </Button>
        )}
      </div>

      <nav className="ph-scroll min-h-0 flex-1 overflow-y-auto px-3 py-2">
        <p className="px-3 pb-2 font-mono text-[11px] font-semibold uppercase tracking-[0.7px] text-[var(--sidebar-section-label)]">
          Preferences
        </p>
        <div className="space-y-1">
          {categories.map((category) => {
            const Icon = category.icon
            const isActive = category.id === activeCategory
            const count =
              category.id === 'accounts'
                ? accountCount
                : category.id === 'mailboxes'
                  ? smartMailboxCount
                  : null

            return (
              <button
                key={category.id}
                type="button"
                onClick={() => onSelect(category.id)}
                style={{
                  backgroundColor: isActive
                    ? `color-mix(in oklab, ${category.accent} 16%, transparent)`
                    : undefined,
                }}
                className={cn(
                  'group flex h-[28px] w-full items-center gap-2 rounded-[5px] px-2 text-left text-[13px] font-medium transition-colors',
                  isActive
                    ? 'text-sidebar-accent-foreground'
                    : 'text-sidebar-foreground/68 hover:bg-sidebar-accent/70 hover:text-sidebar-accent-foreground',
                )}
              >
                <Icon
                  size={17}
                  strokeWidth={1.6}
                  className="shrink-0"
                  style={{ color: category.accent }}
                />
                <span className="min-w-0 flex-1 truncate font-medium">
                  {category.label}
                </span>
                {count !== null && (
                  <span className="font-mono text-[11px] text-sidebar-foreground/50">
                    {count}
                  </span>
                )}
              </button>
            )
          })}
        </div>
      </nav>

      <div className="hidden shrink-0 items-center px-4 py-3 md:flex">
        <span
          className={cn(
            'font-mono text-[10px] font-semibold uppercase tracking-[0.8px]',
            isNightly ? 'text-amber-500' : 'text-sidebar-foreground/40',
          )}
          title={`Posthaste ${releaseChannel} build`}
        >
          {releaseChannel}
        </span>
      </div>
    </aside>
  )
}
