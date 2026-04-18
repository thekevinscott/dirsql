/**
 * Pure helpers for the code-group language URL flag.
 * Extracted from theme/index.ts so they can be unit tested without a DOM.
 */

export const STORAGE_KEY = 'dirsql-preferred-lang'
export const URL_PARAM = 'lang'

/**
 * Read a language preference from a URL's search and/or hash components.
 * Accepts the `window.location.search`/`window.location.hash` strings
 * (with or without their leading `?` / `#`). The query form takes
 * precedence over the hash form. Returns the lowercased language name,
 * or null when no flag is present.
 */
export function parseLanguageFromUrl(
  search: string,
  hash: string
): string | null {
  try {
    const query = new URLSearchParams(search).get(URL_PARAM)
    if (query) return query.trim().toLowerCase()

    const stripped = hash.replace(/^#/, '')
    if (stripped) {
      const fromHash = new URLSearchParams(stripped).get(URL_PARAM)
      if (fromHash) return fromHash.trim().toLowerCase()
    }
  } catch {
    // URL APIs can throw on exotic inputs; fall through to null.
  }
  return null
}
