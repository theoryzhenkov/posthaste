import type { ReactNode } from 'react'

export function MailboxListRow({
  accent,
  icon,
  label,
  sublabel,
  badge,
  onClick,
}: {
  accent: string
  icon: ReactNode
  label: string
  sublabel?: string
  badge?: string | null
  onClick: () => void
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className="group flex min-h-[56px] w-full items-center gap-3 border-b border-border-soft px-4 text-left transition-colors last:border-b-0 hover:bg-[var(--list-hover)]"
    >
      <span
        className="flex size-8 shrink-0 items-center justify-center rounded-[5px] border"
        style={{
          backgroundColor: `color-mix(in oklab, ${accent} 14%, transparent)`,
          borderColor: `color-mix(in oklab, ${accent} 26%, transparent)`,
          color: accent,
        }}
      >
        {icon}
      </span>
      <span className="min-w-0 flex-1">
        <span className="flex items-center gap-2">
          <span className="truncate text-[13px] font-medium text-foreground">
            {label}
          </span>
          {badge && (
            <span
              className="shrink-0 rounded-sm bg-background/80 px-1.5 py-0.5 font-mono text-[9px] uppercase tracking-[0.18em] text-muted-foreground"
              title={badge}
            >
              {badge}
            </span>
          )}
        </span>
        {sublabel && (
          <span className="mt-0.5 block truncate text-[12px] text-muted-foreground">
            {sublabel}
          </span>
        )}
      </span>
      <span className="text-[12px] text-muted-foreground group-hover:text-foreground">
        Edit
      </span>
    </button>
  )
}
