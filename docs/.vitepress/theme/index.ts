import DefaultTheme from 'vitepress/theme'
import type { Theme } from 'vitepress'

const STORAGE_KEY = 'dirsql-preferred-lang'

function applyStoredLanguage() {
  const lang = localStorage.getItem(STORAGE_KEY)
  if (!lang) return

  document.querySelectorAll('.vp-code-group').forEach((group) => {
    const tabs = group.querySelectorAll('.tabs label')
    const blocks = group.querySelectorAll('.blocks > div')

    tabs.forEach((tab, i) => {
      if (tab.textContent?.trim() === lang) {
        // Activate this tab
        const input = group.querySelectorAll('.tabs input')[i] as HTMLInputElement
        if (input && !input.checked) {
          input.checked = true
          // Show corresponding block, hide others
          blocks.forEach((block, j) => {
            ;(block as HTMLElement).style.display = j === i ? '' : 'none'
          })
        }
      }
    })
  })
}

function observeTabClicks() {
  document.addEventListener('change', (e) => {
    const target = e.target as HTMLElement
    if (target.tagName === 'INPUT' && target.closest('.vp-code-group')) {
      const group = target.closest('.vp-code-group')!
      const inputs = group.querySelectorAll('.tabs input')
      const labels = group.querySelectorAll('.tabs label')
      const idx = Array.from(inputs).indexOf(target)
      if (idx >= 0 && labels[idx]) {
        const lang = labels[idx].textContent?.trim()
        if (lang) {
          localStorage.setItem(STORAGE_KEY, lang)
        }
      }
    }
  })
}

export default {
  extends: DefaultTheme,
  enhanceApp() {
    if (typeof window === 'undefined') return

    // Apply on initial load and route changes
    observeTabClicks()

    // Use MutationObserver to apply after VitePress renders content
    const observer = new MutationObserver(() => {
      applyStoredLanguage()
    })

    const tryObserve = () => {
      const content = document.querySelector('.VPContent')
      if (content) {
        observer.observe(content, { childList: true, subtree: true })
        applyStoredLanguage()
      } else {
        requestAnimationFrame(tryObserve)
      }
    }

    if (document.readyState === 'loading') {
      document.addEventListener('DOMContentLoaded', tryObserve)
    } else {
      tryObserve()
    }
  },
} satisfies Theme
