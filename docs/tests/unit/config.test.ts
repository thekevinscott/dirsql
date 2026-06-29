import { describe, expect, it } from 'vitest'
import config from '../../.vitepress/config'

type SidebarItem = { text?: string; link?: string; items?: SidebarItem[] }

describe('vitepress config', () => {
  it('has the expected site title and base path', () => {
    expect(config.title).toBe('dirsql')
    expect(config.base).toBe('/dirsql/')
  })

  // Regression guard: CLI docs live in their own `/cli/` section, not
  // interleaved with the SDK how-to guides (see #179).
  it('keeps CLI pages out of the How-to Guides group', () => {
    const sidebar = config.themeConfig!.sidebar as Record<string, SidebarItem[]>
    const howTo = sidebar['/'].find((group) => group.text === 'How-to Guides')
    const links = (howTo!.items ?? []).map((item) => item.link)
    expect(links).not.toContain('/guide/cli')
    expect(links).not.toContain('/guide/init')
    expect(links).not.toContain('/guide/config')
  })

  // Regression guard: the sidebar must never *replace* itself when entering a
  // section. A path-scoped key (e.g. `/cli/`) swaps the whole tree out, which
  // deletes the other sections and reads as the nav breaking (see #301). There
  // must be exactly one global `/` sidebar.
  it('has a single global sidebar with no replacing path-scoped keys', () => {
    const sidebar = config.themeConfig!.sidebar as Record<string, SidebarItem[]>
    expect(Object.keys(sidebar)).toEqual(['/'])
  })

  // The CLI section is self-contained: its group carries all CLI subpages,
  // alongside (never instead of) the other sections (see #301).
  it('shows every CLI subpage in a self-contained CLI group', () => {
    const sidebar = config.themeConfig!.sidebar as Record<string, SidebarItem[]>
    const groupTexts = sidebar['/'].map((group) => group.text)
    expect(groupTexts).toEqual(['Tutorials', 'How-to Guides', 'CLI', 'Reference'])

    const cli = sidebar['/'].find((group) => group.text === 'CLI')
    const links = (cli!.items ?? []).map((item) => item.link)
    expect(links).toEqual([
      '/cli/',
      '/cli/server',
      '/cli/init',
      '/cli/config',
      '/cli/http-api'
    ])
  })
})
