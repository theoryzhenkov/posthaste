/**
 * A tag rendered as a colored pill with its icon, honoring the user's
 * `settings.tags` appearance overrides (color + icon) and falling back to
 * name-derived defaults.
 *
 */
import { X } from 'lucide-react'

import type { TagAppearance } from '@/data/transport/api'
import { cn } from '@/lib/design/cn'
import { useTagAppearanceLookup } from '@/data/hooks/useTagAppearance'

import { resolveTagStyle } from './model'

interface TagChipProps {
  name: string
  /** When set, ignores the live settings lookup and uses this override (for
   *  settings previews of an unsaved choice). */
  overrideForPreview?: TagAppearance | null
  onRemove?: () => void
  className?: string
}

export function TagChip({
  name,
  overrideForPreview,
  onRemove,
  className,
}: TagChipProps) {
  const lookup = useTagAppearanceLookup()
  const override =
    overrideForPreview !== undefined ? overrideForPreview : lookup(name)
  const { fg, bg, Icon } = resolveTagStyle(name, override)

  return (
    <span
      className={cn(
        'inline-flex h-6 max-w-full items-center gap-1 rounded-full px-2 text-[11px] font-medium',
        className,
      )}
      style={{ color: fg, backgroundColor: bg }}
    >
      <Icon size={11} strokeWidth={2} className="shrink-0" />
      <span className="min-w-0 truncate">{name}</span>
      {onRemove && (
        <button
          type="button"
          aria-label={`Remove ${name}`}
          onClick={onRemove}
          className="ph-focus-ring -mr-1 flex size-4 shrink-0 items-center justify-center rounded-full hover:bg-black/10"
        >
          <X size={11} strokeWidth={2.2} />
        </button>
      )}
    </span>
  )
}
