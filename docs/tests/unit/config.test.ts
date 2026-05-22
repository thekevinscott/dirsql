import { describe, expect, it } from 'vitest'
import config from '../../.vitepress/config'

const themeConfig = config.themeConfig!

type NavItem = { text?: string; link?: string }
type SidebarItem = { text?: string; link?: string; items?: SidebarItem[] }

describe('vitepress config', () => {
  it('has the expected site title and base path', () => {
    expect(config.title).toBe('dirsql')
    expect(config.base).toBe('/dirsql/')
  })
})

describe('CLI navigation', () => {
  const nav = themeConfig.nav as NavItem[]
  const sidebar = themeConfig.sidebar as Record<string, SidebarItem[]>

  it('exposes CLI as a top-level nav entry pointing at the CLI section', () => {
    const cliNav = nav.find((item) => item.text === 'CLI')
    expect(cliNav).toBeDefined()
    expect(cliNav!.link).toBe('/cli/')
  })

  it('uses a path-scoped sidebar with a dedicated /cli/ entry', () => {
    expect(Array.isArray(sidebar)).toBe(false)
    expect(sidebar['/cli/']).toBeDefined()
    expect(sidebar['/']).toBeDefined()
  })

  it('the /cli/ sidebar contains every CLI page as its own section', () => {
    const links = sidebar['/cli/']
      .flatMap((group) => group.items ?? [])
      .map((item) => item.link)
    expect(links).toEqual([
      '/cli/',
      '/cli/server',
      '/cli/init',
      '/cli/config',
      '/cli/http-api'
    ])
  })

  it('does not leave CLI pages in the How-to Guides group', () => {
    const howTo = sidebar['/'].find((group) => group.text === 'How-to Guides')
    const links = (howTo!.items ?? []).map((item) => item.link)
    expect(links).not.toContain('/guide/cli')
    expect(links).not.toContain('/guide/init')
    expect(links).not.toContain('/guide/config')
  })
})
