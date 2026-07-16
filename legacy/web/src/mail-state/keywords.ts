import type { KeywordState } from './types'

/** Derive boolean flags (`isRead`, `isFlagged`) from raw keyword strings. */
export function deriveKeywordState(keywords: string[]): KeywordState {
  return {
    isFlagged: keywords.includes('$flagged'),
    isRead: keywords.includes('$seen'),
    keywords,
  }
}
