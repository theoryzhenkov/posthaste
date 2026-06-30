/**
 * Settings pane for configuring tag appearance: a curated color palette and
 * lucide icon per tag. Tags themselves are keyword-derived; this edits the
 * `settings.tags` presentation overlay.
 *
 * @spec docs/eph/DESIGN-L2-appearance-toml
 * @spec docs/L1-ui#account-settings
 */
import { Tags } from 'lucide-react'
import { useMemo } from 'react'

import type { AppSettings, TagAppearance } from '@/api/types'
import { useMailboxNavigationReadModels } from '@/mailboxNavigationReadModels'
import { cn } from '@/lib/utils'

import { Popover, PopoverContent, PopoverTrigger } from '../ui/popover'
import { TagChip } from '../tags/TagChip'
import { TAG_COLOR_SWATCHES, TAG_ICONS, TAG_ICON_NAMES } from '../tags/model'
import {
  FeedbackBanner,
  SettingsEmptyState,
  SettingsPage,
  SettingsPageHeader,
} from './shared'
import { useTagAppearanceMutation } from './useTagAppearanceMutation'

type IconPatch = Partial<Pick<TagAppearance, 'fg' | 'bg' | 'icon'>>

export function TagsPane({ settings }: { settings: AppSettings | null }) {
  const { tags: discovered } = useMailboxNavigationReadModels()
  const mutation = useTagAppearanceMutation()
  const configured = useMemo(() => settings?.tags ?? [], [settings?.tags])

  // Every tag that exists (carried by a message) or that has a saved override
  // (so a renamed-away tag's config is still reachable to clear), sorted.
  const names = useMemo(() => {
    const set = new Set<string>()
    for (const tag of discovered) set.add(tag.name)
    for (const tag of configured) set.add(tag.name)
    return [...set].sort((a, b) => a.localeCompare(b))
  }, [discovered, configured])

  const overrideFor = (name: string) =>
    configured.find((entry) => entry.name === name)

  function upsert(name: string, patch: IconPatch) {
    const existing = overrideFor(name)
    const merged: TagAppearance = {
      name,
      fg: existing?.fg ?? null,
      bg: existing?.bg ?? null,
      icon: existing?.icon ?? null,
      ...patch,
    }
    const rest = configured.filter((entry) => entry.name !== name)
    const hasAny = merged.fg || merged.bg || merged.icon
    if (!hasAny) {
      mutation.mutate(rest)
      return
    }
    const cleaned: TagAppearance = {
      name,
      ...(merged.fg ? { fg: merged.fg } : {}),
      ...(merged.bg ? { bg: merged.bg } : {}),
      ...(merged.icon ? { icon: merged.icon } : {}),
    }
    mutation.mutate([...rest, cleaned])
  }

  return (
    <SettingsPage>
      <SettingsPageHeader
        title="Tags"
        description="Give your tags a color and an icon. Tags appear on messages once you add them; their look is shared everywhere."
      />

      {mutation.error && (
        <FeedbackBanner tone="error">{mutation.error.message}</FeedbackBanner>
      )}

      {names.length === 0 ? (
        <SettingsEmptyState
          icon={<Tags size={20} strokeWidth={1.7} />}
          title="No tags yet"
          description="Tag a message (press t with a message selected) and it will show up here to customize."
        />
      ) : (
        <div className="flex flex-col divide-y divide-border-soft">
          {names.map((name) => {
            const override = overrideFor(name)
            return (
              <div
                key={name}
                className="flex flex-wrap items-center gap-x-4 gap-y-2 py-3"
              >
                <div className="min-w-0 flex-1">
                  <TagChip name={name} />
                </div>

                <div className="flex items-center gap-1">
                  <SwatchButton
                    selected={!override?.fg && !override?.bg}
                    title="Default color"
                    onClick={() => upsert(name, { fg: null, bg: null })}
                  />
                  {TAG_COLOR_SWATCHES.map((swatch) => (
                    <SwatchButton
                      key={swatch.id}
                      color={swatch.fg}
                      title={swatch.id}
                      selected={
                        override?.fg === swatch.fg && override?.bg === swatch.bg
                      }
                      onClick={() =>
                        upsert(name, { fg: swatch.fg, bg: swatch.bg })
                      }
                    />
                  ))}
                </div>

                <IconPicker
                  value={override?.icon ?? null}
                  onChange={(icon) => upsert(name, { icon })}
                />
              </div>
            )
          })}
        </div>
      )}
    </SettingsPage>
  )
}

function SwatchButton({
  color,
  selected,
  title,
  onClick,
}: {
  color?: string
  selected: boolean
  title: string
  onClick: () => void
}) {
  return (
    <button
      type="button"
      title={title}
      aria-label={title}
      onClick={onClick}
      className={cn(
        'ph-focus-ring size-5 rounded-full border transition-transform hover:scale-110',
        selected
          ? 'border-foreground ring-1 ring-foreground'
          : 'border-border-soft',
      )}
      style={
        color
          ? { backgroundColor: color }
          : {
              backgroundImage:
                'linear-gradient(135deg, transparent 45%, var(--border) 45%, var(--border) 55%, transparent 55%)',
            }
      }
    />
  )
}

function IconPicker({
  value,
  onChange,
}: {
  value: string | null
  onChange: (icon: string | null) => void
}) {
  const Current = (value && TAG_ICONS[value]) || null
  return (
    <Popover>
      <PopoverTrigger asChild>
        <button
          type="button"
          aria-label="Choose tag icon"
          className="ph-focus-ring flex h-7 items-center gap-1.5 rounded-md border border-border-soft px-2 text-[12px] text-muted-foreground transition-colors hover:bg-background/60 hover:text-foreground"
        >
          {Current ? <Current size={14} /> : <span>No icon</span>}
        </button>
      </PopoverTrigger>
      <PopoverContent className="w-60 p-2" align="end">
        <div className="grid grid-cols-8 gap-1">
          <button
            type="button"
            title="No icon"
            aria-label="No icon"
            onClick={() => onChange(null)}
            className={cn(
              'ph-focus-ring flex aspect-square items-center justify-center rounded-md text-[10px] text-muted-foreground hover:bg-[var(--hover-bg)]',
              value === null && 'bg-[var(--hover-bg)] text-foreground',
            )}
          >
            —
          </button>
          {TAG_ICON_NAMES.map((iconName) => {
            const Icon = TAG_ICONS[iconName]
            return (
              <button
                key={iconName}
                type="button"
                title={iconName}
                aria-label={iconName}
                onClick={() => onChange(iconName)}
                className={cn(
                  'ph-focus-ring flex aspect-square items-center justify-center rounded-md text-muted-foreground hover:bg-[var(--hover-bg)] hover:text-foreground',
                  value === iconName && 'bg-[var(--hover-bg)] text-foreground',
                )}
              >
                <Icon size={15} />
              </button>
            )
          })}
        </div>
      </PopoverContent>
    </Popover>
  )
}
