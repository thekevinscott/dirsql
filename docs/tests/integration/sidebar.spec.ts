import { expect, test } from '@playwright/test'

// Regression for #301: entering a section must NOT replace the sidebar.
// The full set of sections has to stay visible on every page, so
// navigation never loses its place. The sections are exactly the four
// Diataxis groups (#353/#387).
const SECTIONS = ['Tutorial', 'How-to Guides', 'Reference', 'Explanation']

async function sidebarSections(page: import('@playwright/test').Page) {
  return await page.evaluate(() =>
    Array.from(document.querySelectorAll('.VPSidebar .group .text'))
      .map((el) => el.textContent?.trim())
      .filter(Boolean)
  )
}

for (const path of [
  'getting-started.html',
  'howto/define-tables.html',
  'reference/cli.html',
  'explanation.html'
]) {
  test(`every section stays in the sidebar on ${path}`, async ({ page }) => {
    await page.goto(`./${path}`)
    await page.waitForSelector('.VPSidebar .group')
    const sections = await sidebarSections(page)
    for (const section of SECTIONS) {
      expect(sections).toContain(section)
    }
  })
}

// The closing sweep (#387): the sidebar shows the four Diataxis groups and
// nothing else -- no `CLI` group, no leftover product-area sections. Group
// titles are the level-0 items; their pages nest below.
test('the sidebar shows exactly the four Diataxis groups', async ({ page }) => {
  await page.goto('./getting-started.html')
  await page.waitForSelector('.VPSidebar .group')
  const groupTitles = await page.evaluate(() =>
    Array.from(
      document.querySelectorAll('.VPSidebar .group .VPSidebarItem.level-0 > .item .text')
    )
      .map((el) => el.textContent?.trim())
      .filter(Boolean)
  )
  expect(groupTitles).toEqual(SECTIONS)
})

// The How-to tree (#376): nine goal-named guides, in the epic's order.
test('the How-to Guides sidebar group lists the nine goal-named guides', async ({ page }) => {
  await page.goto('./howto/define-tables.html')
  await page.waitForSelector('.VPSidebar .group')
  const howtoLinks = await page.evaluate(() => {
    const groups = Array.from(document.querySelectorAll('.VPSidebar .group'))
    const howto = groups.find(
      (g) => g.querySelector('.text')?.textContent?.trim() === 'How-to Guides'
    )
    return Array.from(howto?.querySelectorAll('a') ?? []).map(
      (a) => new URL((a as HTMLAnchorElement).href).pathname
    )
  })
  expect(howtoLinks).toEqual([
    '/dirsql/howto/define-tables.html',
    '/dirsql/howto/columns-from-paths.html',
    '/dirsql/howto/extract-from-contents.html',
    '/dirsql/howto/search-by-meaning.html',
    '/dirsql/howto/skip-files.html',
    '/dirsql/howto/load-extension.html',
    '/dirsql/howto/persist.html',
    '/dirsql/howto/react-to-changes.html',
    '/dirsql/howto/embed.html'
  ])
})

// The Reference tree (#375): the six reference pages plus Migrations.
test('the Reference sidebar group lists all reference pages', async ({ page }) => {
  await page.goto('./reference/cli.html')
  await page.waitForSelector('.VPSidebar .group')
  const referenceLinks = await page.evaluate(() => {
    const groups = Array.from(document.querySelectorAll('.VPSidebar .group'))
    const reference = groups.find(
      (g) => g.querySelector('.text')?.textContent?.trim() === 'Reference'
    )
    return Array.from(reference?.querySelectorAll('a') ?? []).map(
      (a) => new URL((a as HTMLAnchorElement).href).pathname
    )
  })
  expect(referenceLinks).toEqual([
    '/dirsql/reference/cli.html',
    '/dirsql/reference/config.html',
    '/dirsql/reference/hooks.html',
    '/dirsql/reference/columns.html',
    '/dirsql/reference/http-api.html',
    '/dirsql/reference/sdk.html',
    '/dirsql/migrations.html'
  ])
})
