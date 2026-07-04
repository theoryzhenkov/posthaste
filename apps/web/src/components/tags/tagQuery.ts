/**
 * Build the search-language filter that selects every message carrying a tag.
 * `tag` is a spaced-value prefix, so a name with whitespace must be quoted for
 * the parser to keep it as one value (see `query-language/parser`).
 */
export function tagFilterQuery(name: string): string {
  return /\s/.test(name) ? `tag:"${name}"` : `tag:${name}`
}
