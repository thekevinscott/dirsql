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
      { text: 'Reference', link: '/api/' },
      { text: 'Migrations', link: '/migrations' },
      { text: 'GitHub', link: 'https://github.com/thekevinscott/dirsql' }
    ],

    sidebar: [
      {
        text: 'Tutorials',
        items: [
          { text: 'Getting Started', link: '/getting-started' }
        ]
      },
      {
        text: 'How-to Guides',
        items: [
          { text: 'Configuration File', link: '/guide/config' },
          { text: 'Defining Tables', link: '/guide/tables' },
          { text: 'Querying', link: '/guide/querying' },
          { text: 'File Watching', link: '/guide/watching' },
          { text: 'Async API', link: '/guide/async' },
          { text: 'Command-Line Interface', link: '/guide/cli' },
          { text: 'Collaboration with CRDTs', link: '/guide/crdt' }
        ]
      },
      {
        text: 'Reference',
        items: [
          { text: 'API Reference', link: '/api/' },
          { text: 'Migrations', link: '/migrations' }
        ]
      },
    ],

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
