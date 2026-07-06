import { useMemo, useState } from 'react'

import {
  filterAddressSuggestions,
  formatAddressSuggestion,
  insertAddressSuggestion,
  type AddressSuggestionOption,
} from '@/composeAddressSuggestions'

import { Input } from '../ui/input'

export function RecipientSuggestionInput({
  ariaLabel,
  autoFocus = false,
  disabled = false,
  onChange,
  placeholder,
  selectionMode = 'append',
  suggestions,
  value,
}: {
  ariaLabel?: string
  autoFocus?: boolean
  disabled?: boolean
  onChange: (value: string) => void
  placeholder?: string
  /**
   * How a chosen suggestion is committed. Compose recipient fields hold a
   * comma-delimited list, so they `append` the pick to the active token; a
   * single-value field (a rules condition on an address) `replace`s the whole
   * value with the bare email.
   */
  selectionMode?: 'append' | 'replace'
  suggestions: AddressSuggestionOption[]
  value: string
}) {
  const [focused, setFocused] = useState(false)
  const displayedSuggestions = useMemo(
    () => filterAddressSuggestions(suggestions, value),
    [suggestions, value],
  )

  return (
    <div className="relative">
      <Input
        value={value}
        aria-label={ariaLabel}
        autoFocus={autoFocus}
        disabled={disabled}
        onBlur={() => {
          window.setTimeout(() => setFocused(false), 120)
        }}
        onChange={(event) => onChange(event.target.value)}
        onFocus={() => setFocused(true)}
        className="h-7 border-border bg-background/45 text-[13px] text-foreground placeholder:text-muted-foreground/70 focus-visible:ring-ring/25"
        placeholder={placeholder}
      />
      {!disabled && focused && displayedSuggestions.length > 0 && (
        <div className="absolute left-0 right-0 top-8 z-20 max-h-56 overflow-auto rounded-md border border-border bg-popover p-1 text-popover-foreground shadow-lg">
          {displayedSuggestions.map((suggestion) => (
            <button
              key={suggestion.email.toLowerCase()}
              type="button"
              className="grid w-full min-w-0 grid-cols-[minmax(0,1fr)_auto] items-center gap-2 rounded px-2 py-1.5 text-left text-[12px] hover:bg-[var(--hover-bg)]"
              onMouseDown={(event) => event.preventDefault()}
              onClick={() => {
                onChange(
                  selectionMode === 'replace'
                    ? suggestion.email
                    : insertAddressSuggestion(value, suggestion),
                )
                setFocused(false)
              }}
            >
              <span className="min-w-0 truncate">
                {formatAddressSuggestion(suggestion)}
              </span>
              <span className="max-w-32 truncate text-[11px] text-muted-foreground">
                {suggestion.sourceLabel}
              </span>
            </button>
          ))}
        </div>
      )}
    </div>
  )
}
