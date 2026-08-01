/**
 * The field picker both message surfaces share: the offered set, the menu
 * items that draw it, and the visible button that opens it.
 *
 * The two pickers — list columns and detail rows — are one idea over one
 * registry (a set of fields, which are on, toggle one, revert to default), so
 * they are stated once here and differ only in which ids they are handed.
 *
 * Why a BUTTON as well as the right-click menu: right-click was the only way
 * to reach either picker, and nothing on screen said so. A reader who never
 * tries it never learns the columns or the header rows can be changed at all.
 * The context menu stays — it is where a returning user reaches, and it is
 * what a table header does everywhere else — and the button is what tells
 * everyone else the menu exists. Settings carries the same choice a third
 * time, for readers who look for preferences in preferences.
 */
import { useState } from 'react'
import { Check, SlidersHorizontal } from 'lucide-react'

import {
  ContextMenu,
  ContextMenuCheckboxItem,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuSeparator,
  ContextMenuTrigger,
} from '../../ui/overlay/context-menu'
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from '../../ui/overlay/popover'
import { Button } from '../../ui/form/button'
import { getMessageField, type MessageFieldId } from '../fields'

/** The label every surface uses for "put this picker back how it shipped". */
const RESET_LABEL = 'Revert to Default'

export interface FieldPickerOption<Id extends MessageFieldId = MessageFieldId> {
  readonly id: Id
  readonly label: string
  readonly checked: boolean
  /** True when toggling this off is refused — the list needs one column left
   *  to lay out, so the last one standing cannot be turned off. */
  readonly locked: boolean
}

/**
 * What a picker offers: the surface's fields in registry declaration order,
 * each marked with whether it is currently shown.
 *
 * Pure, and exported, because the menu's own items are Radix-portalled and
 * unreachable without a DOM — this is the part of a picker that can be tested
 * directly. Generic over the id type so the list keeps its narrower `ColumnId`
 * all the way through to its toggle callback.
 */
export function fieldPickerOptions<Id extends MessageFieldId>(
  ids: readonly Id[],
  selected: Iterable<MessageFieldId>,
  lockedId?: MessageFieldId | null,
): FieldPickerOption<Id>[] {
  const chosen = new Set(selected)
  return ids.map((id) => ({
    id,
    label: getMessageField(id).label,
    checked: chosen.has(id),
    locked: id === lockedId,
  }))
}

/**
 * The picker as a right-click menu on whatever it wraps — the column header
 * row, the detail field rows.
 *
 * `children` is the trigger rather than a fixed element because the two
 * surfaces right-click on quite different things; only the menu is shared.
 */
export function FieldPickerMenu<Id extends MessageFieldId>({
  children,
  options,
  onToggle,
  onReset,
}: {
  children: React.ReactNode
  options: FieldPickerOption<Id>[]
  onToggle: (id: Id) => void
  onReset: () => void
}) {
  return (
    <ContextMenu>
      <ContextMenuTrigger asChild>{children}</ContextMenuTrigger>
      <ContextMenuContent>
        {options.map((option) => (
          <ContextMenuCheckboxItem
            key={option.id}
            checked={option.checked}
            disabled={option.locked}
            onCheckedChange={() => onToggle(option.id)}
          >
            {option.label}
          </ContextMenuCheckboxItem>
        ))}
        <ContextMenuSeparator />
        <ContextMenuItem onSelect={onReset}>{RESET_LABEL}</ContextMenuItem>
      </ContextMenuContent>
    </ContextMenu>
  )
}

/**
 * The picker's visible trigger: a small icon button opening the same choice as
 * a popover.
 *
 * A popover of plain toggle rows rather than a second Radix menu because the
 * rows must NOT close on click — choosing fields is a run of toggles, and a
 * menu that dismisses itself after each one makes turning three fields on a
 * three-trip job.
 */
export function FieldPickerButton<Id extends MessageFieldId>({
  className,
  label,
  options,
  onToggle,
  onReset,
}: {
  className?: string
  /** Names what is being chosen, on the button's tooltip and atop the
   *  popover — "Choose columns" reads as an offer, an unlabelled icon does
   *  not. */
  label: string
  options: FieldPickerOption<Id>[]
  onToggle: (id: Id) => void
  onReset: () => void
}) {
  const [open, setOpen] = useState(false)

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <Button
          aria-label={label}
          className={className}
          size="icon-xs"
          title={label}
          type="button"
          variant="ghost"
        >
          <SlidersHorizontal size={12} strokeWidth={1.7} />
        </Button>
      </PopoverTrigger>
      <PopoverContent align="end" className="w-52">
        <p className="px-2 pb-1 pt-1 text-meta font-medium uppercase tracking-[0.06em] text-muted-foreground">
          {label}
        </p>
        {options.map((option) => (
          <button
            key={option.id}
            className="flex w-full items-center gap-2 rounded-md px-2 py-1 text-left text-body text-popover-foreground hover:bg-muted disabled:pointer-events-none disabled:opacity-50"
            disabled={option.locked}
            onClick={() => onToggle(option.id)}
            type="button"
          >
            <Check
              className={option.checked ? 'opacity-100' : 'opacity-0'}
              size={13}
              strokeWidth={2}
            />
            {option.label}
          </button>
        ))}
        <button
          className="mt-1 flex w-full items-center rounded-md border-t border-border/60 px-2 py-1 pt-1.5 text-left text-ui text-muted-foreground hover:text-foreground"
          onClick={() => {
            onReset()
            setOpen(false)
          }}
          type="button"
        >
          {RESET_LABEL}
        </button>
      </PopoverContent>
    </Popover>
  )
}
