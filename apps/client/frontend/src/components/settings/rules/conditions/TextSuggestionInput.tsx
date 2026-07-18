/**
 * A text input with a plain-string suggestion dropdown — the generic sibling of
 * the compose `RecipientSuggestionInput` for value types whose suggestions are
 * bare strings (today: `keyword` → the live tag list). Case-insensitive
 * SUBSTRING filtering over the whole value, suggestions shown on focus (even
 * before typing), pick-or-type semantics identical to the address input so the
 * two feel like one mechanism.
 */
import { useMemo, useState } from 'react'

import { Input } from '../../../ui/form/input'

export function TextSuggestionInput({
  ariaLabel,
  className,
  onChange,
  onEnter,
  onPick,
  placeholder,
  suggestions,
  value,
}: {
  ariaLabel?: string
  className?: string
  onChange: (value: string) => void
  /** Pressing Enter with a non-empty value (list-entry commit). */
  onEnter?: (value: string) => void
  /** When set, a suggestion click calls this instead of `onChange` (list-entry
   *  hosts append the pick as an entry). */
  onPick?: (suggestion: string) => void
  placeholder?: string
  suggestions: string[]
  value: string
}) {
  const [focused, setFocused] = useState(false)
  const displayed = useMemo(() => {
    const needle = value.trim().toLowerCase()
    const filtered = needle
      ? suggestions.filter((option) => option.toLowerCase().includes(needle))
      : suggestions
    return filtered.slice(0, 8)
  }, [suggestions, value])

  return (
    <div className="relative">
      <Input
        type="text"
        aria-label={ariaLabel}
        className={className}
        value={value}
        placeholder={placeholder}
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
      />
      {focused && displayed.length > 0 && (
        <div className="absolute left-0 right-0 top-9 z-20 max-h-56 overflow-auto rounded-md border border-border bg-popover p-1 text-popover-foreground shadow-lg">
          {displayed.map((suggestion) => (
            <button
              key={suggestion}
              type="button"
              className="block w-full min-w-0 truncate rounded px-2 py-1.5 text-left text-[12px] hover:bg-[var(--hover-bg)]"
              onMouseDown={(event) => event.preventDefault()}
              onClick={() => {
                if (onPick) {
                  onPick(suggestion)
                } else {
                  onChange(suggestion)
                }
                setFocused(false)
              }}
            >
              {suggestion}
            </button>
          ))}
        </div>
      )}
    </div>
  )
}
