import DefaultTheme from 'vitepress/theme'
import type { Theme } from 'vitepress'

/**
 * Language tab persistence for VitePress code groups.
 *
 * VitePress code groups render as:
 *   .vp-code-group
 *     .tabs
 *       input[type=radio]  (hidden, controls checked state)
 *       label[for=...]     (visible tab)
 *       input
 *       label
 *       ...
 *     .blocks
 *       div.language-python.active   (visible block)
 *       div.language-typescript      (hidden block)
 *       ...
 *
 * Tab switching is driven by click events on the hidden inputs.
 * Active blocks are toggled via the .active CSS class.
 */

import { STORAGE_KEY, parseLanguageFromUrl } from './lang'

const getLanguageFromUrl = (): string | null =>
  parseLanguageFromUrl(window.location.search, window.location.hash)

/**
 * Given a code group element, return parallel arrays of inputs, labels, and blocks.
 */
function getGroupParts(group: Element) {
  const inputs = Array.from(group.querySelectorAll<HTMLInputElement>('.tabs input'))
  const labels = Array.from(group.querySelectorAll<HTMLLabelElement>('.tabs label'))
  const blocks = group.querySelector('.blocks')
  const blockChildren = blocks
    ? Array.from(blocks.children) as HTMLElement[]
    : []
  return { inputs, labels, blocks: blockChildren }
}

function applyStoredLanguage() {
  const lang = localStorage.getItem(STORAGE_KEY)
  if (!lang) return

  document.querySelectorAll('.vp-code-group').forEach((group) => {
    const { inputs, labels, blocks } = getGroupParts(group)

    const idx = labels.findIndex(
      (label) => label.textContent?.trim().toLowerCase() === lang
    )
    if (idx < 0) return

    const input = inputs[idx]
    if (!input || input.checked) return

    // Check the radio button (drives CSS styling via input:checked + label)
    input.checked = true

    // Toggle .active class on blocks (drives visibility)
    blocks.forEach((block, j) => {
      block.classList.toggle('active', j === idx)
    })
  })
}

function observeTabClicks() {
  // VitePress code groups use click events on inputs, not change events.
  window.addEventListener('click', (e) => {
    const el = e.target as HTMLElement
    if (!el.matches('.vp-code-group input')) return

    // input -> .tabs -> .vp-code-group
    const group = el.parentElement?.parentElement
    if (!group) return

    const { inputs, labels } = getGroupParts(group)
    const idx = inputs.indexOf(el as HTMLInputElement)
    if (idx < 0 || !labels[idx]) return

    const lang = labels[idx].textContent?.trim().toLowerCase()
    if (!lang) return

    localStorage.setItem(STORAGE_KEY, lang)

    // Sync all other code groups on the same page
    document.querySelectorAll('.vp-code-group').forEach((otherGroup) => {
      if (otherGroup === group) return
      const other = getGroupParts(otherGroup)
      const otherIdx = other.labels.findIndex(
        (l) => l.textContent?.trim().toLowerCase() === lang
      )
      if (otherIdx < 0) return
      const otherInput = other.inputs[otherIdx]
      if (!otherInput || otherInput.checked) return
      otherInput.checked = true
      other.blocks.forEach((block, j) => {
        block.classList.toggle('active', j === otherIdx)
      })
    })
  })
}

export default {
  extends: DefaultTheme,
  enhanceApp() {
    if (typeof window === 'undefined') return

    // URL flag wins over any previously stored preference, but we only persist
    // once we've confirmed the value matches a real tab -- typos shouldn't
    // poison localStorage. `pendingUrlLang` is consumed the first time a
    // matching render arrives.
    let pendingUrlLang: string | null = getLanguageFromUrl()
    const consumePendingLang = () => {
      if (!pendingUrlLang) return
      const matches = Array.from(
        document.querySelectorAll<HTMLLabelElement>('.vp-code-group .tabs label')
      ).some((l) => l.textContent?.trim().toLowerCase() === pendingUrlLang)
      if (!matches) return
      localStorage.setItem(STORAGE_KEY, pendingUrlLang)
      pendingUrlLang = null
    }
    window.addEventListener('hashchange', () => {
      pendingUrlLang = getLanguageFromUrl()
      consumePendingLang()
      applyStoredLanguage()
    })

    observeTabClicks()

    // Use MutationObserver to apply stored preference after VitePress renders
    const observer = new MutationObserver(() => {
      consumePendingLang()
      applyStoredLanguage()
    })

    const tryObserve = () => {
      const content = document.querySelector('.VPContent')
      if (content) {
        observer.observe(content, { childList: true, subtree: true })
        consumePendingLang()
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
