import { ChevronDown } from 'lucide-react'
import type { Dispatch, SetStateAction } from 'react'

import type { AddressSuggestionOption } from '@/domain/addressSuggestions'
import type { ComposeIntent } from '@/domain/composeIntent'

import {
  optionLabel,
  type ComposeForm,
  type FromAddressOption,
} from './model'
import { Button } from '../../ui/form/button'
import { Input } from '../../ui/form/input'
import { filesFromDataTransfer } from '../attachments/attachments'
import { ComposeLine } from './ComposeLine'
import { RecipientSuggestionInput } from './RecipientSuggestionInput'

interface ComposeFieldsProps {
  displayedFromOptions: FromAddressOption[]
  fieldsDisabled: boolean
  form: ComposeForm
  fromInputFocused: boolean
  fromMenuOpen: boolean
  intentKind: ComposeIntent['kind']
  recipientSuggestions: AddressSuggestionOption[]
  setFromInputFocused: Dispatch<SetStateAction<boolean>>
  setFromMenuOpen: Dispatch<SetStateAction<boolean>>
  onFieldChange: <K extends keyof ComposeForm>(
    field: K,
    value: ComposeForm[K],
  ) => void
  /** Files pasted (Cmd+V) while a recipient/subject field is focused → attachments. */
  onPasteFiles?: (files: File[]) => void
}

export function ComposeFields({
  displayedFromOptions,
  fieldsDisabled,
  form,
  fromInputFocused,
  fromMenuOpen,
  intentKind,
  recipientSuggestions,
  setFromInputFocused,
  setFromMenuOpen,
  onFieldChange,
  onPasteFiles,
}: ComposeFieldsProps) {
  return (
    <div
      className="grid shrink-0 gap-2 border-b border-border/70 px-4 py-3"
      onPaste={(event) => {
        // Only intercept pastes that carry files (a copied file / screenshot)
        // — plain text pastes into the inputs stay untouched.
        const files = filesFromDataTransfer(event.clipboardData)
        if (files.length === 0 || !onPasteFiles) {
          return
        }
        event.preventDefault()
        onPasteFiles(files)
      }}
    >
      <ComposeLine label="From">
        <div className="relative flex min-w-0 items-center gap-1">
          <Input
            value={form.from}
            disabled={fieldsDisabled}
            onBlur={() => {
              window.setTimeout(() => {
                setFromInputFocused(false)
                setFromMenuOpen(false)
              }, 120)
            }}
            onChange={(event) => {
              onFieldChange('from', event.target.value)
              setFromMenuOpen(false)
            }}
            onFocus={() => setFromInputFocused(true)}
            className="h-7 min-w-0 border-border bg-background/45 text-[13px] text-foreground placeholder:text-muted-foreground/70 focus-visible:ring-ring/25"
            placeholder="name@example.com"
          />
          <Button
            type="button"
            variant="ghost"
            size="icon"
            className="size-7 shrink-0 text-muted-foreground hover:bg-[var(--hover-bg)]"
            title="Choose sender"
            disabled={fieldsDisabled}
            onMouseDown={(event) => event.preventDefault()}
            onClick={() => {
              setFromInputFocused(true)
              setFromMenuOpen((open) => !open)
            }}
          >
            <ChevronDown size={15} />
          </Button>
          {(fromMenuOpen || fromInputFocused) &&
            displayedFromOptions.length > 0 && (
              <div className="absolute left-0 right-8 top-8 z-20 max-h-56 overflow-auto rounded-md border border-border bg-popover p-1 text-popover-foreground shadow-lg">
                {displayedFromOptions.map((option) => (
                  <button
                    key={`${option.sourceId}:${option.email}`}
                    type="button"
                    className="grid w-full min-w-0 grid-cols-[minmax(0,1fr)_auto] items-center gap-2 rounded px-2 py-1.5 text-left text-[12px] hover:bg-[var(--hover-bg)]"
                    onMouseDown={(event) => event.preventDefault()}
                    onClick={() => {
                      onFieldChange('from', optionLabel(option))
                      setFromMenuOpen(false)
                      setFromInputFocused(false)
                    }}
                  >
                    <span className="min-w-0 truncate">
                      {optionLabel(option)}
                    </span>
                    <span className="max-w-32 truncate text-[11px] text-muted-foreground">
                      {option.sourceName}
                    </span>
                  </button>
                ))}
              </div>
            )}
        </div>
      </ComposeLine>
      <ComposeLine label="To">
        <RecipientSuggestionInput
          value={form.to}
          autoFocus={intentKind === 'new'}
          disabled={fieldsDisabled}
          onChange={(value) => onFieldChange('to', value)}
          suggestions={recipientSuggestions}
          placeholder="name@example.com"
        />
      </ComposeLine>
      <ComposeLine label="Cc">
        <RecipientSuggestionInput
          value={form.cc}
          disabled={fieldsDisabled}
          onChange={(value) => onFieldChange('cc', value)}
          suggestions={recipientSuggestions}
        />
      </ComposeLine>
      <ComposeLine label="Bcc">
        <RecipientSuggestionInput
          value={form.bcc}
          disabled={fieldsDisabled}
          onChange={(value) => onFieldChange('bcc', value)}
          suggestions={recipientSuggestions}
        />
      </ComposeLine>
      <ComposeLine label="Subject">
        <Input
          value={form.subject}
          disabled={fieldsDisabled}
          onChange={(event) => onFieldChange('subject', event.target.value)}
          className="h-7 border-border bg-background/45 text-[13px] text-foreground placeholder:text-muted-foreground/70 focus-visible:ring-ring/25"
          placeholder="Subject"
        />
      </ComposeLine>
    </div>
  )
}
