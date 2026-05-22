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
})
