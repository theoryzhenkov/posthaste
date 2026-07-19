/**
 * Declarative form-field descriptors: per field, how to read it from form
 * state, when it counts as dirty against the saved baseline, and the wire
 * patch a dirty field emits. Patch assembly built on these never restates
 * stored state — an untouched field always yields `{ kind: 'keep' }`.
 */
import type { Dispatch, SetStateAction } from 'react'

import type { FieldPatch } from '@/gen'

export interface FormField<Form, Value, Patch = unknown> {
  name: string
  read(form: Form): Value
  /** True when `current` no longer matches the saved baseline value. */
  dirtyCompare(current: Value, saved: Value): boolean
  /** Wire patch for a dirty field (untouched fields never reach this). */
  toPatch(current: Value): Patch
}

function trimmedDiffer(current: string, saved: string): boolean {
  return current.trim() !== saved.trim()
}

/** Trimmed text field whose emptied input clears the stored value. */
export function clearableTextField<Form>(
  name: string,
  read: (form: Form) => string,
): FormField<Form, string, FieldPatch<string>> {
  return {
    name,
    read,
    dirtyCompare: trimmedDiffer,
    toPatch: (current) => {
      const trimmed = current.trim()
      return trimmed === '' ? { kind: 'clear' } : { kind: 'set', value: trimmed }
    },
  }
}

/** Trimmed text field that always stores a value when dirty (closed choices,
 * hosts/ports with fallbacks, fields tracked only for the unsaved gate). */
export function textField<Form>(
  name: string,
  read: (form: Form) => string,
): FormField<Form, string, FieldPatch<string>> {
  return {
    name,
    read,
    dirtyCompare: trimmedDiffer,
    toPatch: (current) => ({ kind: 'set', value: current.trim() }),
  }
}

/** The wire patch for one field: keep when untouched, its own patch when
 * dirty. */
export function fieldPatch<Form, Value, Patch>(
  field: FormField<Form, Value, Patch>,
  form: Form,
  saved: Form,
): Patch | { kind: 'keep' } {
  const current = field.read(form)
  return field.dirtyCompare(current, field.read(saved))
    ? field.toPatch(current)
    : { kind: 'keep' }
}

/** Whether any of the fields differs from the saved baseline (the form's
 * unsaved-changes gate). */
export function anyFieldDirty<Form>(
  fields: readonly FormField<Form, string, unknown>[],
  form: Form,
  saved: Form,
): boolean {
  return fields.some((field) =>
    field.dirtyCompare(field.read(form), field.read(saved)),
  )
}

/** Keyed change handlers over a React form state: `set('username')` is the
 * `(value) => onChange((current) => ({ ...current, username: value }))`
 * every editor otherwise restates inline per field. */
export function formFieldSetter<Form extends object>(
  onChange: Dispatch<SetStateAction<Form>>,
): <K extends keyof Form>(name: K) => (value: Form[K]) => void {
  return (name) => (value) =>
    onChange((current) => {
      const next = { ...current }
      next[name] = value
      return next
    })
}

/**
 * Merge a sparse patch into its saved record with keep/set/clear semantics
 * per field: an absent key keeps the saved value, an empty value (undefined
 * or '') clears it, anything else sets it. Returns null when nothing remains
 * set — the record has nothing left to say.
 */
export function mergeSparsePatch<T extends Record<string, string | undefined>>(
  fields: readonly (keyof T & string)[],
  saved: Partial<T>,
  patch: Partial<T>,
): Partial<T> | null {
  const merged: Partial<T> = {}
  let hasAny = false
  for (const field of fields) {
    const value = field in patch ? patch[field] : saved[field]
    if (value) {
      merged[field] = value
      hasAny = true
    }
  }
  return hasAny ? merged : null
}
