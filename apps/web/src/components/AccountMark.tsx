import { buildAccountLogoUrl } from '../api/client'
import { useAuthedBlobUrl } from '../hooks/useAuthedBlobUrl'
import type { AccountAppearance } from '../api/types'
import { cn } from '../lib/utils'

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
  // Logo images are auth-gated daemon resources; the browser can't set the
  // Authorization header on an <img src>, so fetch the bytes ourselves and use
  // the resulting object URL. Until it resolves (or if it fails) we show the
  // letter avatar, which is also the non-image fallback.
  const logoUrl =
    appearance.kind === 'image' ? buildAccountLogoUrl(appearance.imageId) : null
  const { objectUrl } = useAuthedBlobUrl(logoUrl)

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
      {objectUrl ? (
        <img
          alt=""
          className="h-full w-full object-cover"
          draggable={false}
          src={objectUrl}
        />
      ) : (
        accountLetter(appearance)
      )}
    </span>
  )
}
