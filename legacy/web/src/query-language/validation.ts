import { IS_VALUES } from '../queryDefinitions'
import { normalize, prefixDefinition } from './helpers'
import { parseQueryTokens } from './parser'
import type { QueryValidation } from './types'

function isValidIsoDate(value: string): boolean {
  if (!/^\d{4}-\d{2}-\d{2}$/.test(value)) {
    return false
  }
  const date = new Date(`${value}T00:00:00.000Z`)
  return !Number.isNaN(date.getTime()) && date.toISOString().startsWith(value)
}

function validatePrefixedValue(prefix: string, value: string): QueryValidation {
  if (!value.trim()) {
    return { state: 'incomplete', message: `empty value for ${prefix}:` }
  }

  const normalizedPrefix = prefixDefinition(prefix)?.primary
  const normalizedValue = normalize(value)
  switch (normalizedPrefix) {
    case 'is':
      return IS_VALUES.includes(normalizedValue)
        ? { state: 'valid' }
        : { state: 'invalid', message: `unknown is: value: ${value}` }
    case 'has':
      return normalizedValue === 'attachment' ||
        normalizedValue === 'attachments'
        ? { state: 'valid' }
        : { state: 'invalid', message: `unknown has: value: ${value}` }
    case 'date':
      return isValidIsoDate(normalizedValue)
        ? { state: 'valid' }
        : { state: 'invalid', message: `invalid date '${value}'` }
    case 'newer':
    case 'older':
      return /^\d+[dwmy]$/.test(normalizedValue)
        ? { state: 'valid' }
        : {
            state: 'invalid',
            message: `invalid relative date '${value}', expected e.g. 2w`,
          }
    default:
      return { state: 'valid' }
  }
}

export function validateSearchQuery(input: string): QueryValidation {
  const query = input.trim()
  if (!query) {
    return { state: 'incomplete', message: 'empty query' }
  }

  const parsed = parseQueryTokens(query)
  if (!parsed.ok) {
    return parsed.validation
  }

  for (const token of parsed.tokens) {
    if (!token.prefix) {
      continue
    }
    const validation = validatePrefixedValue(token.prefix, token.value)
    if (validation.state !== 'valid') {
      return validation
    }
  }

  return { state: 'valid' }
}
