/**
 * The message header, editable: which rows it shows, in what order, how loud
 * each one is, and whether it prints its key.
 *
 * It lives in settings rather than on the reading pane because the reading
 * pane is CONTENT. Dragging a column header around is chrome answering to the
 * reader; dragging the message you are reading is a different and worse
 * promise, and the in-place picker there is deliberately limited to turning
 * rows on and off.
 *
 * Reordering is a pair of step buttons rather than the drag the column header
 * uses. Drag would have to fight the checkbox and the select sharing each row,
 * it has no keyboard story without extra work, and one step is the unit a
 * reader can undo by pressing the other button. If the list ever grows past a
 * dozen rows this becomes the wrong call.
 */
import { ArrowDown, ArrowUp } from 'lucide-react'

import {
  getMessageField,
  type DetailFieldSetting,
  type MessageFieldEmphasis,
  type MessageFieldId,
} from '../../mail/fields'
import { useDetailFieldConfig } from '../../mail/thread/useFieldConfig'
import { Button } from '../../ui/form/button'
import { Checkbox } from '../../ui/form/checkbox'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '../../ui/form/select'

/** The three steps of the type scale, named for a reader rather than for the
 *  tokens (`--ph-font-size-heading` / `-body` / `-meta`) they resolve to. */
const EMPHASIS_LABELS: Record<MessageFieldEmphasis, string> = {
  heading: 'Heading',
  body: 'Body',
  meta: 'Small',
}

export function DetailRowEditor({
  fields,
  offered,
}: {
  /** The reader's rows, in their order. */
  fields: DetailFieldSetting[]
  /** Every field the detail surface can show, in registry order. */
  offered: MessageFieldId[]
}) {
  const { toggleDetailField, updateDetailField, moveDetailField } =
    useDetailFieldConfig()

  const chosen = new Set(fields.map((field) => field.id))

  return (
    <div className="space-y-1">
      {fields.map((field, index) => (
        <div
          className="flex flex-wrap items-center gap-2 rounded-md border border-border-soft px-2 py-1.5"
          key={field.id}
        >
          <label className="flex min-w-0 flex-1 items-center gap-2 text-body text-foreground">
            <Checkbox
              checked
              onCheckedChange={() => toggleDetailField(field.id)}
            />
            <span className="truncate">{getMessageField(field.id).label}</span>
          </label>

          <Select
            onValueChange={(value) =>
              updateDetailField(field.id, {
                emphasis: value as MessageFieldEmphasis,
              })
            }
            value={field.emphasis}
          >
            <SelectTrigger
              aria-label={`${getMessageField(field.id).label} emphasis`}
              className="h-7 w-[104px] rounded-md border-border bg-background text-ui shadow-none"
              size="sm"
            >
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {(
                Object.keys(EMPHASIS_LABELS) as MessageFieldEmphasis[]
              ).map((emphasis) => (
                <SelectItem key={emphasis} value={emphasis}>
                  {EMPHASIS_LABELS[emphasis]}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>

          {/* Per field, not global: a subject reading `Subject: Re: numbers`
              says less than the subject alone, while a bare list of addresses
              cannot say whether it is To or CC. */}
          <label className="flex items-center gap-1.5 text-ui text-muted-foreground">
            <Checkbox
              checked={field.showLabel}
              onCheckedChange={() =>
                updateDetailField(field.id, { showLabel: !field.showLabel })
              }
            />
            Label
          </label>

          <div className="flex items-center">
            <Button
              aria-label={`Move ${getMessageField(field.id).label} up`}
              disabled={index === 0}
              onClick={() => moveDetailField(field.id, -1)}
              size="icon-xs"
              type="button"
              variant="ghost"
            >
              <ArrowUp size={13} strokeWidth={1.7} />
            </Button>
            <Button
              aria-label={`Move ${getMessageField(field.id).label} down`}
              disabled={index === fields.length - 1}
              onClick={() => moveDetailField(field.id, 1)}
              size="icon-xs"
              type="button"
              variant="ghost"
            >
              <ArrowDown size={13} strokeWidth={1.7} />
            </Button>
          </div>
        </div>
      ))}

      {/* The rest of the registry, offered but not shown. They carry no
          controls because emphasis and label belong to a row that exists;
          turning one on gives it the presentation its field declares. */}
      <div className="grid gap-2 pt-2 sm:grid-cols-2">
        {offered
          .filter((id) => !chosen.has(id))
          .map((id) => (
            <label
              className="flex items-center gap-2 text-body text-muted-foreground"
              key={id}
            >
              <Checkbox
                checked={false}
                onCheckedChange={() => toggleDetailField(id)}
              />
              {getMessageField(id).label}
            </label>
          ))}
      </div>
    </div>
  )
}
