import type { UIEvent as ReactUIEvent } from 'react'

import type { PaletteRow, SearchCandidate } from '@/app/input/palette/search/types'

import { CommandItem, CommandList } from '@/components/ui/overlay/command'
import { commandPaletteEntryValue, NO_COMMAND_PALETTE_SELECTION } from './model'

export function CommandPaletteList({
  itemRowsLength,
  rows,
  onRunCandidate,
  onScroll,
  onSelectNone,
}: {
  itemRowsLength: number
  rows: PaletteRow[]
  onRunCandidate: (candidate: SearchCandidate) => void
  onScroll: (event: ReactUIEvent<HTMLDivElement>) => void
  onSelectNone: () => void
}) {
  return (
    <CommandList
      className="ph-scroll max-h-[min(440px,calc(100vh-170px))] px-0 py-1.5"
      onScroll={onScroll}
    >
      {itemRowsLength > 0 && (
        <CommandItem
          aria-hidden="true"
          value={NO_COMMAND_PALETTE_SELECTION}
          className="hidden"
          onSelect={onSelectNone}
        />
      )}
      {rows.length === 0 ? (
        <div className="py-10 text-center text-sm text-muted-foreground">
          No results. Try a different query.
        </div>
      ) : (
        rows.map((row) => renderRow(row, onRunCandidate))
      )}
    </CommandList>
  )
}

function renderRow(
  row: PaletteRow,
  onRunCandidate: (candidate: SearchCandidate) => void,
) {
  switch (row.kind) {
    case 'section':
      return (
        <div
          key={row.id}
          className="px-4 py-2 font-mono text-[10px] font-semibold tracking-[0.22em] text-muted-foreground/80 uppercase"
        >
          {row.label}
        </div>
      )
    case 'item': {
      const entry = row.candidate.entry
      const isDisabled = entry.disabled === true
      // Disabled rows stay visible and highlightable (discoverability) but are
      // inert: onSelect no-ops so Enter/click skip them.
      return (
        <CommandItem
          key={row.id}
          value={commandPaletteEntryValue(row.candidate)}
          aria-disabled={isDisabled || undefined}
          className={
            isDisabled
              ? 'mx-0 px-4 py-2.5 text-muted-foreground/60 data-[selected=true]:bg-[var(--hover-bg)]'
              : 'mx-0 px-4 py-2.5 text-foreground data-[selected=true]:bg-[var(--hover-bg)]'
          }
          onSelect={
            isDisabled ? undefined : () => onRunCandidate(row.candidate)
          }
        >
          <span className="flex size-4 shrink-0 items-center justify-center">
            {entry.icon}
          </span>
          <span className="min-w-0 flex-1 truncate">{entry.label}</span>
          {isDisabled && entry.disabledReason && (
            <span className="max-w-[14rem] truncate text-[12px] text-muted-foreground/70 italic">
              {entry.disabledReason}
            </span>
          )}
          {!isDisabled && entry.subtitle && (
            <span className="max-w-[14rem] truncate text-[12px] text-muted-foreground">
              {entry.subtitle}
            </span>
          )}
          {entry.shortcut && (
            <kbd className="ml-2 shrink-0 rounded border border-border/60 bg-muted/50 px-1.5 py-0.5 font-mono text-[10px] text-muted-foreground">
              {entry.shortcut}
            </kbd>
          )}
        </CommandItem>
      )
    }
    case 'loading':
      return (
        <div key={row.id} className="px-4 py-2 text-sm text-muted-foreground">
          {row.label}
        </div>
      )
    case 'empty':
      return (
        <div key={row.id} className="px-4 py-2 text-sm text-muted-foreground">
          {row.label}
        </div>
      )
    case 'error':
      return (
        <div key={row.id} className="px-4 py-2 text-sm text-destructive">
          {row.message}
        </div>
      )
  }
}
