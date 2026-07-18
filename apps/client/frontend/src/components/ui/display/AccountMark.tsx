import type { AccountAppearance } from '../../../data/transport/api/index'
import { useAccountLogoUrl } from '@/data/transport/blobs'
import { cn } from '../../../lib/cn'

interface AccountMarkProps {
  appearance: AccountAppearance
  className?: string
}

function accountHueColor(colorHue: number): string {
  return `oklch(0.60 0.12 ${colorHue})`
}

function accountLetter(appearance: AccountAppearance): string {
  return appearance.initials.trim().charAt(0).toUpperCase() || '?'
}

export function AccountMark({ appearance, className }: AccountMarkProps) {
  const color = accountHueColor(appearance.colorHue)
  const accountLogoUrl = useAccountLogoUrl()
  // Logo images are served by the authenticated asset route; the token rides
  // in the URL, so a plain <img src> works. The letter avatar is the
  // non-image (and load-failure) fallback via the surrounding span.
  const logoUrl =
    appearance.kind === 'image' ? accountLogoUrl(appearance.imageId) : null

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
      {logoUrl ? (
        <img
          alt=""
          className="h-full w-full object-cover"
          draggable={false}
          src={logoUrl}
        />
      ) : (
        accountLetter(appearance)
      )}
    </span>
  )
}
