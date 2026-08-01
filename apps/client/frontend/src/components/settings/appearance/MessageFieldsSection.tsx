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
 *
 * The two halves are not symmetric, and should not be. A column carries its
 * order and its width on the header itself, where dragging is expected; a
 * detail row carries emphasis, its label and its place, none of which belong
 * on the reading pane — so the header half is the fuller editor
 * (`DetailRowEditor`) and this file keeps the columns' plain checklist.
 */
import { ALL_COLUMNS } from '../../mail/thread/columns'
import { fieldPickerOptions } from '../../mail/thread/fieldPicker'
import {
  useColumnConfig,
  useDetailFieldConfig,
} from '../../mail/thread/useFieldConfig'
import { fieldsForSurface } from '../../mail/fields'
import { Button } from '../../ui/form/button'
import { Checkbox } from '../../ui/form/checkbox'
import { SettingsSection } from '../panel/shared'
import { DetailRowEditor } from './DetailRowEditor'

const DETAIL_FIELDS = fieldsForSurface('detail')

export function MessageFieldsSection() {
  const { columns, toggleColumn, resetColumns } = useColumnConfig()
  const { detailFields, resetDetailFields } = useDetailFieldConfig()

  const columnOptions = fieldPickerOptions(
    ALL_COLUMNS,
    columns,
    // The table needs one column to lay out, so the last one standing shows as
    // locked rather than as a click that does nothing.
    columns.length === 1 ? columns[0] : null,
  )

  return (
    <>
      <SettingsSection
        actions={<RevertButton onClick={resetColumns} />}
        title="List columns"
      >
        <p className="text-ui leading-5 text-muted-foreground">
          Columns in the message list. Drag a column header to reorder, or its
          edge to resize.
        </p>
        <div className="grid gap-2 sm:grid-cols-2">
          {columnOptions.map((option) => (
            <label
              className="flex items-center gap-2 text-body text-foreground has-disabled:opacity-50"
              key={option.id}
            >
              <Checkbox
                checked={option.checked}
                disabled={option.locked}
                onCheckedChange={() => toggleColumn(option.id)}
              />
              {option.label}
            </label>
          ))}
        </div>
      </SettingsSection>

      <SettingsSection
        actions={<RevertButton onClick={resetDetailFields} />}
        title="Message header"
      >
        <p className="text-ui leading-5 text-muted-foreground">
          Rows above the message you are reading, in the order they appear. A
          field with nothing in it — a message with no CC — shows no row at
          all.
        </p>
        <DetailRowEditor fields={detailFields} offered={DETAIL_FIELDS} />
      </SettingsSection>
    </>
  )
}

function RevertButton({ onClick }: { onClick: () => void }) {
  return (
    <Button
      className="text-muted-foreground"
      onClick={onClick}
      size="xs"
      type="button"
      variant="ghost"
    >
      Revert to default
    </Button>
  )
}
