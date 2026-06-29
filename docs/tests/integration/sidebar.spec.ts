import { expect, test } from '@playwright/test'

// Regression for #301: entering the CLI section must NOT replace the sidebar.
// The full set of sections has to stay visible on every page, including
// `/cli/*`, so navigation never loses its place.
const SECTIONS = ['Tutorials', 'How-to Guides', 'CLI', 'Reference']

async function sidebarSections(page: import('@playwright/test').Page) {
  return await page.evaluate(() =>
    Array.from(document.querySelectorAll('.VPSidebar .group .text'))
      .map((el) => el.textContent?.trim())
      .filter(Boolean)
  )
}

for (const path of ['guide/tables.html', 'cli/index.html', 'cli/server.html']) {
  test(`every section stays in the sidebar on ${path}`, async ({ page }) => {
    await page.goto(`./${path}`)
    await page.waitForSelector('.VPSidebar .group')
    const sections = await sidebarSections(page)
    for (const section of SECTIONS) {
      expect(sections).toContain(section)
    }
  })
}

test('the CLI sidebar group lists all CLI subpages', async ({ page }) => {
  await page.goto('./cli/index.html')
  await page.waitForSelector('.VPSidebar .group')
  const cliLinks = await page.evaluate(() => {
    const groups = Array.from(document.querySelectorAll('.VPSidebar .group'))
    const cli = groups.find(
      (g) => g.querySelector('.text')?.textContent?.trim() === 'CLI'
    )
    return Array.from(cli?.querySelectorAll('a') ?? []).map(
      (a) => new URL((a as HTMLAnchorElement).href).pathname
    )
  })
  expect(cliLinks).toEqual([
    '/dirsql/cli/',
    '/dirsql/cli/server.html',
    '/dirsql/cli/init.html',
    '/dirsql/cli/config.html',
    '/dirsql/cli/http-api.html'
  ])
})
