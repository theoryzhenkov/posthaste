import {
  useEffect,
  useMemo,
  useRef,
  type Dispatch,
  type SetStateAction,
} from 'react'
import { useMutation } from '@tanstack/react-query'

import { useCommands } from '@/data'
import type { AccountAppearance } from '@/gen'
import { AccountMark } from '../../../ui/display/AccountMark'
import { buildAccountAppearanceInput, formFieldSetter } from '../../forms'
import { FeedbackBanner, Field } from '../../panel/shared'
import type { AccountFormState } from '../../panel/types'

const accountHueGradient =
  'linear-gradient(90deg, oklch(0.68 0.17 0), oklch(0.68 0.17 45), oklch(0.68 0.17 90), oklch(0.68 0.17 145), oklch(0.68 0.17 205), oklch(0.68 0.17 260), oklch(0.68 0.17 315), oklch(0.68 0.17 360))'

/** Change key for the debounced autosave: fires only when the normalized
 * appearance actually differs from what was last saved. */
function accountAppearanceSignature(appearance: AccountAppearance): string {
  const imagePart = appearance.kind === 'image' ? appearance.imageId : ''
  return `${appearance.kind}:${appearance.initials}:${appearance.colorHue}:${imagePart}`
}

/**
 * Letter + hue editor with debounced autosave: an existing account's
 * appearance patch posts as `updateAccount` shortly after the last edit; the
 * refreshed answer arrives through the ordinary invalidation cycle.
 */
export function AccountAppearanceFields({
  accountId,
  form,
  onChange,
}: {
  accountId: string | null
  form: AccountFormState
  onChange: Dispatch<SetStateAction<AccountFormState>>
}) {
  const commands = useCommands()
  const setField = formFieldSetter(onChange)
  const previewAppearance = useMemo(
    () => buildAccountAppearanceInput(form),
    [form],
  )
  const appearanceKey = accountAppearanceSignature(previewAppearance)
  const savedAppearanceKeyRef = useRef<string | null>(
    accountId ? appearanceKey : null,
  )
  const saveAppearanceMutation = useMutation({
    mutationFn: (currentForm: AccountFormState) =>
      commands.run({
        updateAccount: {
          accountId: accountId!,
          appearance: buildAccountAppearanceInput(currentForm),
        },
      }),
    onSuccess: (_accepted, currentForm) => {
      savedAppearanceKeyRef.current = accountAppearanceSignature(
        buildAccountAppearanceInput(currentForm),
      )
    },
  })
  const { error: saveAppearanceError, mutate: saveAppearance } =
    saveAppearanceMutation

  useEffect(() => {
    if (!accountId || appearanceKey === savedAppearanceKeyRef.current) {
      return
    }

    const timeout = window.setTimeout(() => {
      saveAppearance(form)
    }, 350)
    return () => window.clearTimeout(timeout)
  }, [accountId, appearanceKey, saveAppearance, form])

  return (
    <div className="grid gap-4 sm:grid-cols-[auto_1fr]">
      <AccountMark
        appearance={previewAppearance}
        className="size-12 rounded-md text-[15px]"
      />

      <div className="min-w-0 space-y-3">
        <div className="grid gap-3 sm:grid-cols-[96px_1fr]">
          <Field
            label="Letter"
            value={form.appearanceInitials}
            onChange={(value) =>
              setField('appearanceInitials')(value.toUpperCase().slice(0, 1))
            }
          />
          <label className="grid gap-1.5 text-[13px]">
            <span className="flex items-center justify-between text-[12px] font-medium text-muted-foreground">
              <span>Color</span>
              <span className="font-mono">{form.appearanceColorHue}°</span>
            </span>
            <input
              type="range"
              min={0}
              max={360}
              step={1}
              value={form.appearanceColorHue}
              onChange={(event) =>
                setField('appearanceColorHue')(Number(event.target.value))
              }
              className="ph-hue-range h-4 w-full cursor-pointer appearance-none rounded-full border border-border-soft bg-transparent accent-primary"
              style={{ background: accountHueGradient }}
            />
          </label>
        </div>
        {saveAppearanceError && (
          <FeedbackBanner tone="error">
            {saveAppearanceError.message}
          </FeedbackBanner>
        )}
      </div>
    </div>
  )
}
