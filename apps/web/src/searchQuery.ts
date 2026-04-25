import { validateSearchQuery, type QueryValidation } from './queryLanguage'

export interface PreparedServerSearchQuery {
  query: string | undefined
  validation: QueryValidation
  isBlocked: boolean
}

export function normalizeAppliedSearchQuery(value: string): string {
  return value.trim().replace(/\s+/g, ' ')
}

export function normalizeValidAppliedSearchQuery(value: string): string | null {
  const prepared = prepareServerSearchQuery(value)
  if (prepared.isBlocked) {
    return null
  }
  return prepared.query ?? ''
}

export function prepareServerSearchQuery(
  value: string | undefined,
): PreparedServerSearchQuery {
  const normalized = normalizeAppliedSearchQuery(value ?? '')
  if (!normalized) {
    return {
      query: undefined,
      validation: { state: 'valid' },
      isBlocked: false,
    }
  }

  const validation = validateSearchQuery(normalized)
  if (validation.state !== 'valid') {
    return {
      query: undefined,
      validation,
      isBlocked: true,
    }
  }

  return {
    query: normalized,
    validation,
    isBlocked: false,
  }
}
