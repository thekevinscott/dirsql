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
          text: 'Tutorials',
          items: [
            { text: 'Getting Started', link: '/getting-started' }
          ]
        },
        {
          text: 'How-to Guides',
          items: [
            { text: 'Defining Tables', link: '/guide/tables' },
            { text: 'Querying', link: '/guide/querying' },
            { text: 'File Watching', link: '/guide/watching' },
            { text: 'Persistence', link: '/guide/persistence' },
            { text: 'Async API', link: '/guide/async' },
            { text: 'Collaboration with CRDTs', link: '/guide/crdt' }
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
            { text: 'API Reference', link: '/api/' },
            { text: 'Migrations', link: '/migrations' }
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
