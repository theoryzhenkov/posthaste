import { useMemo, useState } from 'react'

import {
  filterAddressSuggestions,
  formatAddressSuggestion,
  insertAddressSuggestion,
  type AddressSuggestionOption,
} from '@/domain/addressSuggestions'

import { Input } from '../../ui/form/input'

export function RecipientSuggestionInput({
  ariaLabel,
  autoFocus = false,
  disabled = false,
  onChange,
  onEnter,
  onPick,
  placeholder,
  selectionMode = 'append',
  suggestions,
  value,
}: {
  ariaLabel?: string
  autoFocus?: boolean
  disabled?: boolean
  onChange: (value: string) => void
  /**
   * Pressing Enter with a non-empty value. Used by list-entry hosts (an `in`
   * condition's adder) to commit the typed draft as one entry.
   */
  onEnter?: (value: string) => void
  /**
   * When set, choosing a suggestion calls THIS with the bare email instead of
   * routing through `onChange` — a list-entry host appends the pick as an
   * entry rather than editing the text.
   */
  onPick?: (email: string) => void
  placeholder?: string
  /**
   * How a chosen suggestion is committed. Compose recipient fields hold a
   * comma-delimited list, so they `append` the pick to the active token; a
   * single-value field (a rules condition on an address) `replace`s the whole
   * value with the bare email. The filter needle follows the same split:
   * append-mode filters on the token being typed, replace-mode on the whole
   * value (a single-value input must never be comma-tokenized).
   */
  selectionMode?: 'append' | 'replace'
  suggestions: AddressSuggestionOption[]
  value: string
}) {
  const [focused, setFocused] = useState(false)
  const displayedSuggestions = useMemo(
    () =>
      filterAddressSuggestions(
        suggestions,
        value,
        8,
        selectionMode === 'replace' ? 'whole' : 'token',
      ),
    [suggestions, value, selectionMode],
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
        onKeyDown={(event) => {
          if (event.key === 'Enter' && onEnter && value.trim().length > 0) {
            event.preventDefault()
            onEnter(value)
          }
        }}
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
                if (onPick) {
                  onPick(suggestion.email)
                } else {
                  onChange(
                    selectionMode === 'replace'
                      ? suggestion.email
                      : insertAddressSuggestion(value, suggestion),
                  )
                }
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
