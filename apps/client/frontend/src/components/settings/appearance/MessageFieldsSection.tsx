/**
 * "Message fields": which columns the message list shows and which rows the
 * message header shows, in one place.
 *
 * This is the third way to reach the same two pickers — the others are the
 * right-click menu and the button on each surface — and it exists because the
 * first two are only findable while looking at the thing they configure. A
 * reader who wonders "can I see the CC?" looks in preferences.
 *
 * It can be ONE section covering both surfaces because both are driven by the
 * one message-field registry: same fields, same labels, same store. It lives
 * under Appearance because that is where a reader looks for what the app puts
 * on screen, and because the alternative — its own rail category — would buy
 * a vocabulary entry and rail wiring for a single section.
 */
import { ALL_COLUMNS } from '../../mail/thread/columns'
import {
  fieldPickerOptions,
  type FieldPickerOption,
} from '../../mail/thread/fieldPicker'
import {
  useColumnConfig,
  useDetailFieldConfig,
} from '../../mail/thread/useFieldConfig'
import { fieldsForSurface, type MessageFieldId } from '../../mail/fields'
import { Button } from '../../ui/form/button'
import { Checkbox } from '../../ui/form/checkbox'
import { SettingsSection } from '../panel/shared'

const DETAIL_FIELDS = fieldsForSurface('detail')

export function MessageFieldsSection() {
  const { columns, toggleColumn, resetColumns } = useColumnConfig()
  const { detailFields, toggleDetailField, resetDetailFields } =
    useDetailFieldConfig()

  return (
    <>
      <FieldGroup
        description="Columns in the message list. Drag a column header to reorder, or its edge to resize."
        options={fieldPickerOptions(
          ALL_COLUMNS,
          columns,
          // The table needs one column to lay out, so the last one standing
          // shows as locked rather than as a click that does nothing.
          columns.length === 1 ? columns[0] : null,
        )}
        onReset={resetColumns}
        onToggle={toggleColumn}
        title="List columns"
      />
      <FieldGroup
        description="Rows above the message you are reading. A field with nothing in it — a message with no CC — shows no row at all."
        options={fieldPickerOptions(DETAIL_FIELDS, detailFields)}
        onReset={resetDetailFields}
        onToggle={toggleDetailField}
        title="Message header"
      />
    </>
  )
}

/** One surface's fields. Generic over the id so the list keeps its narrower
 *  `ColumnId` through to `toggleColumn`. */
function FieldGroup<Id extends MessageFieldId>({
  description,
  options,
  onReset,
  onToggle,
  title,
}: {
  description: string
  options: FieldPickerOption<Id>[]
  onReset: () => void
  onToggle: (id: Id) => void
  title: string
}) {
  return (
    <SettingsSection
      actions={
        <Button
          className="text-muted-foreground"
          onClick={onReset}
          size="xs"
          type="button"
          variant="ghost"
        >
          Revert to default
        </Button>
      }
      title={title}
    >
      <p className="text-ui leading-5 text-muted-foreground">{description}</p>
      <div className="grid gap-2 sm:grid-cols-2">
        {options.map((option) => (
          <label
            className="flex items-center gap-2 text-body text-foreground has-disabled:opacity-50"
            key={option.id}
          >
            <Checkbox
              checked={option.checked}
              disabled={option.locked}
              onCheckedChange={() => onToggle(option.id)}
            />
            {option.label}
          </label>
        ))}
      </div>
    </SettingsSection>
  )
}
