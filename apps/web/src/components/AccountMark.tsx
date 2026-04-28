import { buildAccountLogoUrl } from '../api/client'
import type { AccountAppearance } from '../api/types'
import { cn } from '../lib/utils'

interface AccountMarkProps {
  appearance: AccountAppearance
  className?: string
  imageUrl?: string | null
}

function accountHueColor(colorHue: number): string {
  return `oklch(0.60 0.12 ${colorHue})`
}

function accountLetter(appearance: AccountAppearance): string {
  return appearance.initials.trim().charAt(0).toUpperCase() || '?'
}

export function AccountMark({
  appearance,
  className,
  imageUrl,
}: AccountMarkProps) {
  const color = accountHueColor(appearance.colorHue)
  const resolvedImageUrl =
    imageUrl !== undefined
      ? imageUrl
      : appearance.kind === 'image'
        ? buildAccountLogoUrl(appearance.imageId)
        : undefined

  return (
    <span
      className={cn(
        'flex size-8 shrink-0 items-center justify-center overflow-hidden rounded-[5px] border font-mono text-[11px] font-semibold text-white shadow-[inset_0_1px_0_rgb(255_255_255/0.18)]',
        className,
      )}
      style={{
        backgroundColor: color,
        borderColor: `color-mix(in oklab, ${color} 78%, black)`,
      }}
    >
      {resolvedImageUrl ? (
        <img
          alt=""
          className="h-full w-full object-cover"
          draggable={false}
          src={resolvedImageUrl}
        />
      ) : (
        accountLetter(appearance)
      )}
    </span>
  )
}
