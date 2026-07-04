import { defineConfig } from 'vitepress'

export default defineConfig({
  title: 'dirsql',
  description: 'Ephemeral SQL index over a local directory. Watches a filesystem, ingests structured files into an in-memory SQLite database, and exposes a SQL query interface.',
  base: '/dirsql/',

  themeConfig: {
    search: {
      provider: 'local'
    },

    nav: [
      { text: 'Getting Started', link: '/getting-started' },
      { text: 'Guide', link: '/guide/tables' },
      { text: 'CLI', link: '/cli/' },
      { text: 'Reference', link: '/api/' },
      { text: 'Migrations', link: '/migrations' },
      { text: 'GitHub', link: 'https://github.com/thekevinscott/dirsql' }
    ],

    // A single global sidebar shown on every page. The CLI section is a
    // self-contained group with all its subpages -- but it never *replaces*
    // the rest of the nav. There is intentionally no path-scoped (`/cli/`)
    // key: a path-scoped sidebar swaps the whole tree out, which deletes the
    // other sections when you enter CLI (see #301). Keep one sidebar.
    sidebar: {
      '/': [
        {
          text: 'Tutorial',
          items: [
            { text: 'Your first dirsql database', link: '/getting-started' }
          ]
        },
        {
          text: 'How-to Guides',
          items: [
            { text: 'Define tables for your files', link: '/howto/define-tables' },
            { text: 'Derive columns from file paths', link: '/howto/columns-from-paths' },
            { text: 'Extract rows from file contents', link: '/howto/extract-from-contents' },
            { text: 'Search documents by meaning', link: '/howto/search-by-meaning' },
            { text: "Skip files you don't want indexed", link: '/howto/skip-files' },
            { text: 'Load a SQLite extension', link: '/howto/load-extension' },
            { text: 'Keep the index across restarts', link: '/howto/persist' },
            { text: 'React to file changes', link: '/howto/react-to-changes' },
            { text: 'Embed dirsql in your application', link: '/howto/embed' }
          ]
        },
        {
          text: 'CLI',
          items: [
            { text: 'Overview & Installation', link: '/cli/' },
            { text: 'Running the Server', link: '/cli/server' },
            { text: 'Generating a Config (`init`)', link: '/cli/init' },
            { text: 'Configuration File', link: '/cli/config' },
            { text: 'HTTP API', link: '/cli/http-api' }
          ]
        },
        {
          text: 'Reference',
          items: [
            { text: 'CLI', link: '/reference/cli' },
            { text: 'Configuration File', link: '/reference/config' },
            { text: 'Command Hooks', link: '/reference/hooks' },
            { text: 'Virtual Columns & Glob Captures', link: '/reference/columns' },
            { text: 'HTTP API', link: '/reference/http-api' },
            { text: 'SDK', link: '/reference/sdk' },
            { text: 'Migrations', link: '/migrations' }
          ]
        },
        {
          text: 'Explanation',
          items: [
            { text: 'How dirsql thinks', link: '/explanation' }
          ]
        }
      ]
    },

    outline: {
      level: [2, 3],
      label: 'On this page'
    },

    socialLinks: [
      { icon: 'github', link: 'https://github.com/thekevinscott/dirsql' }
    ],

    footer: {
      message: 'Released under the MIT License.',
      copyright: 'Copyright 2024-present'
    }
  }
})
