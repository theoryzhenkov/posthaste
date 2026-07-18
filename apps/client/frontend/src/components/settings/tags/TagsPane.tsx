/**
 * Settings pane for managing tags. Tags are keyword-derived (no registry): this
 * pane edits their presentation overlay (`settings.tags` — a curated color and
 * lucide icon per tag) AND offers global Rename / Delete, which re-keyword every
 * carrier through the ordinary `tag:<name>` search surface (see
 * {@link ./useTagMaintenance}).
 *
 */
import { Check, Loader2, Pencil, Tags, Trash2, X } from 'lucide-react'
import { useMemo, useState } from 'react'

import type { AppSettings, TagAppearance } from '@/data/transport/api'
import { useMailboxNavigationReadModels } from '@/data/models/mailboxNavigation'
import { cn } from '@/lib/design/cn'

import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
  AlertDialogTrigger,
} from '../../ui/overlay/alert-dialog'
import { Input } from '../../ui/form/input'
import { Popover, PopoverContent, PopoverTrigger } from '../../ui/overlay/popover'
import { TagChip } from '../../mail/tags/TagChip'
import { TAG_COLOR_SWATCHES, TAG_ICONS, TAG_ICON_NAMES } from '../../mail/tags/model'
import {
  FeedbackBanner,
  SettingsEmptyState,
  SettingsPage,
  SettingsPageHeader,
} from '../panel/shared'
import { classifyRename } from './tagMaintenance'
import { useTagAppearanceMutation } from './useTagAppearanceMutation'
import { useTagMaintenance } from './useTagMaintenance'

type IconPatch = Partial<Pick<TagAppearance, 'fg' | 'bg' | 'icon'>>

function normalizeTagName(value: string): string | null {
  const normalized = value.trim().replace(/\s+/g, ' ')
  if (!normalized || normalized.startsWith('$') || normalized.includes('/')) {
    return null
  }
  return normalized
}

export function TagsPane({ settings }: { settings: AppSettings | null }) {
  const { tags: discovered } = useMailboxNavigationReadModels()
  const mutation = useTagAppearanceMutation()
  const maintenance = useTagMaintenance()
  const configured = useMemo(() => settings?.tags ?? [], [settings?.tags])

  // A pending rename that collided with an existing tag — held here until the
  // merge is confirmed (or cancelled). See requestRename below.
  const [pendingMerge, setPendingMerge] = useState<{
    oldName: string
    newName: string
  } | null>(null)

  // Every tag that exists (carried by a message) or that has a saved override
  // (so a renamed-away tag's config is still reachable to clear), sorted.
  const names = useMemo(() => {
    const set = new Set<string>()
    for (const tag of discovered) set.add(tag.name)
    for (const tag of configured) set.add(tag.name)
    return [...set].sort((a, b) => a.localeCompare(b))
  }, [discovered, configured])

  const countFor = (name: string) =>
    discovered.find((tag) => tag.name === name)?.totalMessages ?? 0

  const overrideFor = (name: string) =>
    configured.find((entry) => entry.name === name)

  function upsert(name: string, patch: IconPatch) {
    const existing = overrideFor(name)
    const merged: TagAppearance = {
      name,
      fg: existing?.fg,
      bg: existing?.bg,
      icon: existing?.icon,
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

  // A rename is applied directly unless the new name collides with an existing
  // tag, in which case it is a MERGE and we confirm first.
  function requestRename(oldName: string, rawNewName: string) {
    const newName = normalizeTagName(rawNewName)
    if (!newName) return
    const kind = classifyRename(oldName, newName, names)
    if (kind === 'noop') return
    if (kind === 'merge') {
      setPendingMerge({ oldName, newName })
      return
    }
    void maintenance.rename(oldName, newName, configured)
  }

  const busy = maintenance.isRunning

  return (
    <SettingsPage>
      <SettingsPageHeader
        title="Tags"
        description="Give your tags a color and an icon, or rename and delete them everywhere they're used. Tags appear on messages once you add them."
      />

      {mutation.error && (
        <FeedbackBanner tone="error">{mutation.error.message}</FeedbackBanner>
      )}

      {maintenance.progress && (
        <p className="flex items-center gap-2 rounded-md border border-border-soft bg-background/60 px-3 py-2 text-[12px] text-muted-foreground">
          <Loader2 size={14} className="animate-spin" />
          {maintenance.progress.action === 'rename'
            ? 'Renaming'
            : 'Deleting'}{' '}
          &ldquo;{maintenance.progress.tag}&rdquo; — {maintenance.progress.done}
          /{maintenance.progress.total || '…'} messages
        </p>
      )}

      {names.length === 0 ? (
        <SettingsEmptyState
          icon={<Tags size={20} strokeWidth={1.7} />}
          title="No tags yet"
          description="Tag a message (press t with a message selected) and it will show up here to customize."
        />
      ) : (
        <div className="flex flex-col divide-y divide-border-soft">
          {names.map((name) => (
            <TagRow
              key={name}
              name={name}
              count={countFor(name)}
              override={overrideFor(name)}
              disabled={busy}
              onUpsert={upsert}
              onRename={requestRename}
              onDelete={() => void maintenance.remove(name, configured)}
            />
          ))}
        </div>
      )}

      <AlertDialog
        open={pendingMerge !== null}
        onOpenChange={(open) => {
          if (!open) setPendingMerge(null)
        }}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Merge tags?</AlertDialogTitle>
            <AlertDialogDescription>
              A tag named &ldquo;{pendingMerge?.newName}&rdquo; already exists.
              Renaming &ldquo;{pendingMerge?.oldName}&rdquo; will merge its{' '}
              {pendingMerge ? countFor(pendingMerge.oldName) : 0} messages into
              it.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction
              onClick={() => {
                if (pendingMerge) {
                  void maintenance.rename(
                    pendingMerge.oldName,
                    pendingMerge.newName,
                    configured,
                  )
                }
                setPendingMerge(null)
              }}
            >
              Merge tags
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </SettingsPage>
  )
}

function TagRow({
  name,
  count,
  override,
  disabled,
  onUpsert,
  onRename,
  onDelete,
}: {
  name: string
  count: number
  override: TagAppearance | undefined
  disabled: boolean
  onUpsert: (name: string, patch: IconPatch) => void
  onRename: (oldName: string, newName: string) => void
  onDelete: () => void
}) {
  const [draft, setDraft] = useState<string | null>(null)
  const isEditing = draft !== null

  function commit() {
    if (draft !== null) onRename(name, draft)
    setDraft(null)
  }

  return (
    <div className="flex flex-wrap items-center gap-x-4 gap-y-2 py-3">
      <div className="min-w-0 flex-1">
        {isEditing ? (
          <form
            className="flex items-center gap-1.5"
            onSubmit={(event) => {
              event.preventDefault()
              commit()
            }}
          >
            <Input
              autoFocus
              value={draft ?? ''}
              onChange={(event) => setDraft(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === 'Escape') setDraft(null)
              }}
              aria-label={`Rename tag ${name}`}
              className="h-7 max-w-56"
            />
            <IconButton
              type="submit"
              title="Save"
              aria-label="Save tag name"
              icon={<Check size={15} />}
            />
            <IconButton
              type="button"
              title="Cancel"
              aria-label="Cancel rename"
              icon={<X size={15} />}
              onClick={() => setDraft(null)}
            />
          </form>
        ) : (
          <TagChip name={name} />
        )}
      </div>

      {!isEditing && (
        <>
          <div className="flex items-center gap-1">
            <SwatchButton
              selected={!override?.fg && !override?.bg}
              title="Default color"
              onClick={() => onUpsert(name, { fg: undefined, bg: undefined })}
            />
            {TAG_COLOR_SWATCHES.map((swatch) => (
              <SwatchButton
                key={swatch.id}
                color={swatch.fg}
                title={swatch.id}
                selected={
                  override?.fg === swatch.fg && override?.bg === swatch.bg
                }
                onClick={() => onUpsert(name, { fg: swatch.fg, bg: swatch.bg })}
              />
            ))}
          </div>

          <IconPicker
            value={override?.icon ?? null}
            onChange={(icon) => onUpsert(name, { icon: icon ?? undefined })}
          />

          <div className="flex items-center gap-1">
            <IconButton
              type="button"
              title="Rename tag"
              aria-label={`Rename tag ${name}`}
              disabled={disabled}
              icon={<Pencil size={14} />}
              onClick={() => setDraft(name)}
            />
            <AlertDialog>
              <AlertDialogTrigger asChild>
                <IconButton
                  type="button"
                  title="Delete tag"
                  aria-label={`Delete tag ${name}`}
                  disabled={disabled}
                  icon={<Trash2 size={14} />}
                />
              </AlertDialogTrigger>
              <AlertDialogContent>
                <AlertDialogHeader>
                  <AlertDialogTitle>Delete tag?</AlertDialogTitle>
                  <AlertDialogDescription>
                    This removes &ldquo;{name}&rdquo; from {count}{' '}
                    {count === 1 ? 'message' : 'messages'} and forgets its
                    appearance. This can&rsquo;t be undone.
                  </AlertDialogDescription>
                </AlertDialogHeader>
                <AlertDialogFooter>
                  <AlertDialogCancel>Cancel</AlertDialogCancel>
                  <AlertDialogAction variant="destructive" onClick={onDelete}>
                    Delete tag
                  </AlertDialogAction>
                </AlertDialogFooter>
              </AlertDialogContent>
            </AlertDialog>
          </div>
        </>
      )}
    </div>
  )
}

function IconButton({
  icon,
  title,
  disabled,
  type = 'button',
  onClick,
  ...rest
}: {
  icon: React.ReactNode
  title: string
  disabled?: boolean
  type?: 'button' | 'submit'
  onClick?: () => void
} & Pick<React.AriaAttributes, 'aria-label'>) {
  return (
    <button
      type={type}
      title={title}
      disabled={disabled}
      onClick={onClick}
      className="ph-focus-ring flex size-7 items-center justify-center rounded-md border border-border-soft text-muted-foreground transition-colors hover:bg-background/60 hover:text-foreground disabled:pointer-events-none disabled:opacity-40"
      {...rest}
    >
      {icon}
    </button>
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
