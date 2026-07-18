import { SPACED_VALUE_PREFIXES } from './definitions'
import { isWhitespace, prefixDefinition } from './scan'
import type { QueryValidation } from './types'

export function parseQueryTokens(input: string):
  | {
      ok: true
      tokens: Array<{ prefix: string | null; value: string }>
    }
  | {
      ok: false
      validation: QueryValidation
    } {
  const tokens: Array<{ prefix: string | null; value: string }> = []
  const chars = [...input]
  let index = 0

  while (index < chars.length) {
    while (index < chars.length && isWhitespace(chars[index] ?? '')) {
      index += 1
    }
    if (index >= chars.length) {
      break
    }

    if (
      chars[index] === '-' &&
      index + 1 < chars.length &&
      !isWhitespace(chars[index + 1] ?? '')
    ) {
      index += 1
    }

    const start = index
    let colonIndex: number | null = null
    while (index < chars.length && !isWhitespace(chars[index] ?? '')) {
      if (chars[index] === ':') {
        colonIndex = index
        break
      }
      index += 1
    }

    if (colonIndex === null) {
      index = start
      tokens.push({ prefix: null, value: scanTokenValue(chars, index).value })
      index = scanTokenValue(chars, index).end
      continue
    }

    const prefix = input.slice(start, colonIndex).toLowerCase()
    const definition = prefixDefinition(prefix)
    if (!definition) {
      return {
        ok: false,
        validation: {
          state: 'invalid',
          message: `unknown search prefix: ${prefix}`,
        },
      }
    }

    index = colonIndex + 1
    while (index < chars.length && isWhitespace(chars[index] ?? '')) {
      index += 1
    }

    let value: string
    if (SPACED_VALUE_PREFIXES.has(prefix)) {
      if (chars[index] === '"') {
        const scanned = scanTokenValue(chars, index)
        value = scanned.value
        index = scanned.end
      } else if (startsKnownPrefixTokenAt(chars, index)) {
        value = ''
      } else {
        const valueStart = index
        while (index < chars.length) {
          if (startsKnownPrefixAt(chars, index)) {
            break
          }
          index += 1
        }
        value = input.slice(valueStart, index).trim()
      }
    } else {
      const scanned = scanTokenValue(chars, index)
      value = scanned.value
      index = scanned.end
    }

    tokens.push({ prefix, value })
  }

  return { ok: true, tokens }
}

function scanTokenValue(
  chars: string[],
  start: number,
): { value: string; end: number } {
  if (chars[start] === '"') {
    let end = start + 1
    while (end < chars.length && chars[end] !== '"') {
      end += 1
    }
    const value = chars.slice(start + 1, end).join('')
    return { value, end: end < chars.length ? end + 1 : end }
  }

  let end = start
  while (end < chars.length && !isWhitespace(chars[end] ?? '')) {
    end += 1
  }
  return { value: chars.slice(start, end).join(''), end }
}

function startsKnownPrefixTokenAt(chars: string[], position: number): boolean {
  if (position >= chars.length) {
    return false
  }

  let index = position
  if (chars[index] === '-') {
    index += 1
  }

  const start = index
  while (index < chars.length && !isWhitespace(chars[index] ?? '')) {
    if (chars[index] === ':') {
      return prefixDefinition(chars.slice(start, index).join('')) !== undefined
    }
    index += 1
  }
  return false
}

function startsKnownPrefixAt(chars: string[], position: number): boolean {
  if (position >= chars.length || !isWhitespace(chars[position] ?? '')) {
    return false
  }

  let index = position
  while (index < chars.length && isWhitespace(chars[index] ?? '')) {
    index += 1
  }
  if (chars[index] === '-') {
    index += 1
  }

  const start = index
  while (index < chars.length && !isWhitespace(chars[index] ?? '')) {
    if (chars[index] === ':') {
      return prefixDefinition(chars.slice(start, index).join('')) !== undefined
    }
    index += 1
  }
  return false
}
