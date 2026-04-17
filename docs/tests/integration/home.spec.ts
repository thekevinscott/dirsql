import { expect, test } from '@playwright/test'

test('home page renders the site title', async ({ page }) => {
  await page.goto('./')
  await expect(page).toHaveTitle(/dirsql/)
})
