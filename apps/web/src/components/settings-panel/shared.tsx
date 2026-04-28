/** Reusable form and display primitives for the settings panel. */

import type { AccountOverview } from '../../api/types'
import { cn } from '../../lib/utils'
import { ArrowLeft } from 'lucide-react'
import { Button } from '../ui/button'
import { Input } from '../ui/input'

export function SettingsPage({
  children,
  className,
}: {
  children: React.ReactNode
  className?: string
}) {
  return (
    <div className={cn('mx-auto flex max-w-[760px] flex-col', className)}>
      {children}
    </div>
  )
}

export function SettingsBackButton({
  children,
  onClick,
  ariaLabel,
}: {
  children: React.ReactNode
  onClick: () => void
  ariaLabel: string
}) {
  return (
    <Button
      aria-label={ariaLabel}
      size="sm"
      variant="ghost"
      type="button"
      onClick={onClick}
      className="mb-6 h-7 self-start rounded-md px-2 text-[12px] text-muted-foreground hover:bg-[var(--list-hover)] hover:text-foreground"
    >
      <ArrowLeft size={14} strokeWidth={1.5} />
      {children}
    </Button>
  )
}

export function SettingsPageHeader({
  title,
  description,
  leading,
  meta,
  actions,
}: {
  title: string
  description?: string
  leading?: React.ReactNode
  meta?: React.ReactNode
  actions?: React.ReactNode
}) {
  return (
    <header className="mb-8 flex flex-col gap-4 sm:flex-row sm:items-start sm:justify-between">
      <div className="flex min-w-0 items-center gap-3">
        {leading}
        <div className="min-w-0">
          <h1 className="truncate text-[24px] font-semibold leading-tight text-foreground">
            {title}
          </h1>
          {meta ? (
            <div className="mt-1">{meta}</div>
          ) : description ? (
            <p className="mt-2 max-w-[620px] text-[13px] leading-6 text-muted-foreground">
              {description}
            </p>
          ) : null}
        </div>
      </div>
      {actions && (
        <div className="flex shrink-0 flex-wrap items-center gap-1">
          {actions}
        </div>
      )}
    </header>
  )
}

export function SettingsSection({
  title,
  children,
  actions,
  tone = 'default',
  className,
}: {
  title: string
  children: React.ReactNode
  actions?: React.ReactNode
  tone?: 'default' | 'danger'
  className?: string
}) {
  return (
    <section
      className={cn('grid gap-3 py-5 md:grid-cols-[140px_1fr]', className)}
    >
      <div className="flex min-w-0 items-start justify-between gap-3 md:block">
        <h2
          className={cn(
            'text-[12px] font-semibold uppercase tracking-[0.08em]',
            tone === 'danger' ? 'text-destructive' : 'text-muted-foreground',
          )}
        >
          {title}
        </h2>
        {actions && <div className="md:mt-3">{actions}</div>}
      </div>
      <div className="min-w-0 space-y-3">{children}</div>
    </section>
  )
}

export function SettingsFooter({
  children,
  className,
}: {
  children: React.ReactNode
  className?: string
}) {
  return (
    <div className={cn('grid gap-3 pt-2 md:grid-cols-[140px_1fr]', className)}>
      <div aria-hidden />
      <div className="min-w-0 space-y-3">{children}</div>
    </div>
  )
}

export function SettingsList({
  title,
  actions,
  children,
  className,
}: {
  title: string
  actions?: React.ReactNode
  children: React.ReactNode
  className?: string
}) {
  return (
    <div
      className={cn(
        'mt-7 overflow-hidden rounded-lg border border-border-soft bg-bg-elev/45',
        className,
      )}
    >
      <div className="flex min-h-[48px] items-center justify-between gap-3 border-b border-border-soft px-4">
        <h2 className="text-[13px] font-semibold text-foreground">{title}</h2>
        {actions && <div className="flex items-center gap-2">{actions}</div>}
      </div>
      {children}
    </div>
  )
}

export function SettingsEmptyState({
  icon,
  title,
  description,
  action,
}: {
  icon: React.ReactNode
  title: string
  description: string
  action?: React.ReactNode
}) {
  return (
    <div className="flex min-h-[220px] flex-col items-center justify-center rounded-lg border border-dashed border-border-soft bg-bg-elev/45 px-6 text-center">
      <div className="text-muted-foreground/40">{icon}</div>
      <div className="mt-4">
        <p className="text-[13px] font-medium">{title}</p>
        <p className="mt-1 text-[13px] text-muted-foreground">{description}</p>
      </div>
      {action && <div className="mt-4">{action}</div>}
    </div>
  )
}

/** Labeled text input field. */
export function Field({
  label,
  value,
  placeholder,
  onChange,
  type,
  disabled,
}: {
  label: string
  value: string | number
  placeholder?: string
  onChange: (value: string) => void
  type?: string
  disabled?: boolean
}) {
  return (
    <label className="grid gap-1.5 text-[13px]">
      <span className="text-[12px] font-medium text-muted-foreground">
        {label}
      </span>
      <Input
        className="h-8 rounded-md border-border bg-background text-[13px] shadow-none"
        type={type}
        value={value}
        placeholder={placeholder}
        disabled={disabled}
        onChange={(event) => onChange(event.target.value)}
      />
    </label>
  )
}

const STATUS_DOT_COLOR: Record<AccountOverview['status'], string> = {
  ready: 'bg-emerald-500',
  syncing: 'bg-blue-500',
  degraded: 'bg-amber-500',
  authError: 'bg-rose-500',
  offline: 'bg-orange-500',
  disabled: 'bg-zinc-400',
}

/** Colored status dot for account health indicators. */
export function StatusDot({
  status,
  className,
}: {
  status: AccountOverview['status']
  className?: string
}) {
  return (
    <span
      aria-hidden
      title={status}
      className={cn(
        'inline-block h-2 w-2 shrink-0 rounded-full',
        STATUS_DOT_COLOR[status],
        className,
      )}
    />
  )
}

export function FeedbackBanner({
  tone,
  children,
}: {
  tone: 'success' | 'error'
  children: React.ReactNode
}) {
  return (
    <p
      className={cn(
        'rounded-md border px-3 py-2 text-[12px]',
        tone === 'success'
          ? 'border-emerald-500/20 bg-emerald-500/5 text-emerald-700'
          : 'border-destructive/20 bg-destructive/5 text-destructive',
      )}
    >
      {children}
    </p>
  )
}
