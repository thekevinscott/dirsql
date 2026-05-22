import { expect, test } from '@playwright/test'

test('CLI is a top-level nav entry that opens the CLI section', async ({ page }) => {
  await page.goto('./')

  const cliNav = page.locator('.VPNavBar a.VPNavBarMenuLink', { hasText: 'CLI' })
  await expect(cliNav).toHaveAttribute('href', /\/cli\/$/)

  await cliNav.click()
  await expect(page).toHaveURL(/\/cli\/$/)
})

test('the CLI sidebar contains only CLI pages', async ({ page }) => {
  await page.goto('./cli/')

  const sidebarLinks = page.locator('.VPSidebar .VPSidebarItem a.link')
  const hrefs = await sidebarLinks.evaluateAll((els) =>
    els.map((el) => (el as HTMLAnchorElement).getAttribute('href'))
  )

  expect(hrefs.length).toBeGreaterThan(0)
  for (const href of hrefs) {
    expect(href).toMatch(/\/cli\//)
  }
})
