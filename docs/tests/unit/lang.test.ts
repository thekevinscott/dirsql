import { describe, expect, it } from 'vitest'
import { parseLanguageFromUrl, STORAGE_KEY, URL_PARAM } from '../../.vitepress/theme/lang'

describe('parseLanguageFromUrl', () => {
  it('reads ?lang= from the query string', () => {
    expect(parseLanguageFromUrl('?lang=rust', '')).toBe('rust')
    expect(parseLanguageFromUrl('?lang=python', '')).toBe('python')
    expect(parseLanguageFromUrl('?lang=typescript', '')).toBe('typescript')
  })

  it('reads #lang= from the hash fragment (with or without leading #)', () => {
    expect(parseLanguageFromUrl('', '#lang=rust')).toBe('rust')
    expect(parseLanguageFromUrl('', 'lang=python')).toBe('python')
  })

  it('prefers the query string over the hash fragment', () => {
    expect(parseLanguageFromUrl('?lang=rust', '#lang=python')).toBe('rust')
  })

  it('normalises values: lowercases and trims whitespace', () => {
    expect(parseLanguageFromUrl('?lang=RUST', '')).toBe('rust')
    expect(parseLanguageFromUrl('?lang=%20Python%20', '')).toBe('python')
  })

  it('returns null when the flag is absent', () => {
    expect(parseLanguageFromUrl('', '')).toBeNull()
    expect(parseLanguageFromUrl('?foo=bar', '')).toBeNull()
    expect(parseLanguageFromUrl('', '#section-heading')).toBeNull()
  })

  it('exposes stable storage/param keys', () => {
    expect(STORAGE_KEY).toBe('dirsql-preferred-lang')
    expect(URL_PARAM).toBe('lang')
  })
})
