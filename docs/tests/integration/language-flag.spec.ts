import { expect, test } from '@playwright/test'

const STORAGE_KEY = 'dirsql-preferred-lang'
const PAGE = 'getting-started.html'

async function readActiveLanguages(page: import('@playwright/test').Page) {
  return await page.evaluate(() =>
    Array.from(document.querySelectorAll('.vp-code-group .blocks > .active'))
      .map((el) =>
        Array.from(el.classList).find((c) => c.startsWith('language-')) ?? null
      )
  )
}

async function readStoredLanguage(page: import('@playwright/test').Page) {
  return await page.evaluate((key) => localStorage.getItem(key), STORAGE_KEY)
}

test.describe('code-group language URL flag', () => {
  test.beforeEach(async ({ page }) => {
    // Start every test from a clean persistence state. Navigate once to
    // an origin page so localStorage is scoped correctly, then clear.
    // We intentionally don't use `addInitScript` for this -- it would
    // also fire on in-test navigations and wipe state the test just set.
    await page.goto('./')
    await page.evaluate(() => localStorage.clear())
  })

  for (const lang of ['rust', 'python', 'typescript'] as const) {
    test(`?lang=${lang} activates ${lang} blocks and persists`, async ({ page }) => {
      await page.goto(`${PAGE}?lang=${lang}`)
      await page.waitForSelector(`.vp-code-group .blocks > .active.language-${lang}`)

      const active = await readActiveLanguages(page)
      expect(active).toContain(`language-${lang}`)
      expect(await readStoredLanguage(page)).toBe(lang)
    })
  }

  test('#lang=rust hash form works the same as the query form', async ({ page }) => {
    await page.goto(`${PAGE}#lang=rust`)
    await page.waitForSelector('.vp-code-group .blocks > .active.language-rust')

    const active = await readActiveLanguages(page)
    expect(active).toContain('language-rust')
    expect(await readStoredLanguage(page)).toBe('rust')
  })

  test('unknown language does not poison localStorage', async ({ page }) => {
    await page.goto(`${PAGE}?lang=cobol`)
    await page.waitForSelector('.vp-code-group .blocks > .active')

    expect(await readStoredLanguage(page)).toBeNull()
  })

  test('language choice carries across pages', async ({ page }) => {
    await page.goto(`${PAGE}?lang=rust`)
    await page.waitForSelector('.vp-code-group .blocks > .active.language-rust')
    expect(await readStoredLanguage(page)).toBe('rust')

    // Navigate to the home page and wait for the Rust block to be activated
    // by the MutationObserver that applies the stored preference.
    await page.goto('./')
    await page.waitForSelector('.vp-code-group .blocks > .active.language-rust')
  })
})
