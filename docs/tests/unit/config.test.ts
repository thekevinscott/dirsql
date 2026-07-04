import { describe, expect, it } from 'vitest'
import config from '../../.vitepress/config'

type SidebarItem = { text?: string; link?: string; items?: SidebarItem[] }
type NavItem = { text?: string; link?: string }

describe('vitepress config', () => {
  it('has the expected site title and base path', () => {
    expect(config.title).toBe('dirsql')
    expect(config.base).toBe('/dirsql/')
  })

  // The closing sweep (#387): the nav mirrors the four Diataxis groups
  // exactly (plus the GitHub link). No product-area tabs -- the old `CLI`
  // tab is gone (#353).
  it('shows exactly the four Diataxis groups plus GitHub in the nav', () => {
    const nav = config.themeConfig!.nav as NavItem[]
    expect(nav.map((item) => item.text)).toEqual([
      'Tutorial',
      'How-to Guides',
      'Reference',
      'Explanation',
      'GitHub'
    ])
    expect(nav.map((item) => item.link)).toEqual([
      '/getting-started',
      '/howto/define-tables',
      '/reference/cli',
      '/explanation',
      'https://github.com/thekevinscott/dirsql'
    ])
  })

  // The sidebar mirrors the same four groups, in Diataxis order. No `CLI`
  // group -- type is the only organizational axis (#353/#387).
  it('has exactly the four Diataxis groups in the sidebar', () => {
    const sidebar = config.themeConfig!.sidebar as Record<string, SidebarItem[]>
    const groupTexts = sidebar['/'].map((group) => group.text)
    expect(groupTexts).toEqual(['Tutorial', 'How-to Guides', 'Reference', 'Explanation'])
  })

  // Regression guard: the sidebar must never *replace* itself when entering a
  // section. A path-scoped key (e.g. `/howto/`) swaps the whole tree out,
  // which deletes the other sections and reads as the nav breaking (see
  // #301). There must be exactly one global `/` sidebar.
  it('has a single global sidebar with no replacing path-scoped keys', () => {
    const sidebar = config.themeConfig!.sidebar as Record<string, SidebarItem[]>
    expect(Object.keys(sidebar)).toEqual(['/'])
  })

  // The Tutorial group holds the single lesson (#377).
  it('lists the tutorial in the Tutorial group', () => {
    const sidebar = config.themeConfig!.sidebar as Record<string, SidebarItem[]>
    const tutorial = sidebar['/'].find((group) => group.text === 'Tutorial')
    const links = (tutorial!.items ?? []).map((item) => item.link)
    expect(links).toEqual(['/getting-started'])
  })

  // The How-to tree (#376): nine goal-named guides, in the epic's order
  // (#353 blessed target tree).
  it('lists the nine goal-named how-to guides in the How-to Guides group', () => {
    const sidebar = config.themeConfig!.sidebar as Record<string, SidebarItem[]>
    const howTo = sidebar['/'].find((group) => group.text === 'How-to Guides')
    const links = (howTo!.items ?? []).map((item) => item.link)
    expect(links).toEqual([
      '/howto/define-tables',
      '/howto/columns-from-paths',
      '/howto/extract-from-contents',
      '/howto/search-by-meaning',
      '/howto/skip-files',
      '/howto/load-extension',
      '/howto/persist',
      '/howto/react-to-changes',
      '/howto/embed'
    ])
  })

  // The Reference group is the canonical fact tree (#375): the six reference
  // pages plus the Migrations include, in look-up order.
  it('lists the six reference pages plus Migrations in the Reference group', () => {
    const sidebar = config.themeConfig!.sidebar as Record<string, SidebarItem[]>
    const reference = sidebar['/'].find((group) => group.text === 'Reference')
    const links = (reference!.items ?? []).map((item) => item.link)
    expect(links).toEqual([
      '/reference/cli',
      '/reference/config',
      '/reference/hooks',
      '/reference/columns',
      '/reference/http-api',
      '/reference/sdk',
      '/migrations'
    ])
  })

  // The Explanation group holds the single "how dirsql thinks" page (#374).
  it('lists the explanation page in the Explanation group', () => {
    const sidebar = config.themeConfig!.sidebar as Record<string, SidebarItem[]>
    const explanation = sidebar['/'].find((group) => group.text === 'Explanation')
    const links = (explanation!.items ?? []).map((item) => item.link)
    expect(links).toEqual(['/explanation'])
  })
})
