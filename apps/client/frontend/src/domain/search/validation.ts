import { parseIsoDate } from '../time'
import { IS_VALUES } from './definitions'
import { normalize, prefixDefinition } from './scan'
import { parseQueryTokens } from './parser'
import type { QueryValidation } from './types'

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
      return parseIsoDate(normalizedValue)
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
