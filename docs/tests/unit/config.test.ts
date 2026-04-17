import { describe, expect, it } from 'vitest'
import config from '../../.vitepress/config'

describe('vitepress config', () => {
  it('has the expected site title and base path', () => {
    expect(config.title).toBe('dirsql')
    expect(config.base).toBe('/dirsql/')
  })
})
