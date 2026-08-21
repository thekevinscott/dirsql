import { defineConfig } from 'vitepress'

export default defineConfig({
  title: 'dirsql',
  description: 'Ephemeral SQL index over a local directory. Watches a filesystem, ingests structured files into an ephemeral SQLite database, and exposes a SQL query interface.',
  base: '/dirsql/',

  // Vite blocks dev-server requests whose Host header isn't allowlisted, so
  // `pnpm dev --host` reached by a network hostname (e.g. a Tailscale
  // `*.ts.net` name) 400s. Allow extra hosts from the environment so a
  // personal hostname never has to be committed:
  //   VITEPRESS_ALLOWED_HOSTS=my.host.ts.net pnpm dev --host
  vite: {
    server: {
      allowedHosts: process.env.VITEPRESS_ALLOWED_HOSTS?.split(',') ?? []
    }
  },

  themeConfig: {
    search: {
      provider: 'local'
    },

    // The nav mirrors the four Diataxis groups exactly (#353/#387). Type is
    // the only organizational axis -- no product-area tabs (the old `CLI`
    // tab is gone).
    nav: [
      { text: 'Tutorial', link: '/getting-started' },
      { text: 'How-to Guides', link: '/howto/define-tables' },
      { text: 'Reference', link: '/reference/cli' },
      { text: 'Explanation', link: '/explanation' },
      { text: 'Plugins', link: '/plugins' },
      { text: 'GitHub', link: 'https://github.com/thekevinscott/dirsql' }
    ],

    // A single global sidebar shown on every page, mirroring the four
    // Diataxis groups. There is intentionally no path-scoped (e.g.
    // `/howto/`) key: a path-scoped sidebar swaps the whole tree out, which
    // deletes the other sections when you enter one (see #301). Keep one
    // sidebar.
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
            { text: 'Query files without a config', link: '/howto/query-without-config' },
            { text: 'Define tables for your files', link: '/howto/define-tables' },
            { text: 'Derive columns from file paths', link: '/howto/columns-from-paths' },
            { text: 'Extract rows from file contents', link: '/howto/extract-from-contents' },
            { text: 'Parse your files into columns', link: '/howto/parse-files-into-columns' },
            { text: 'Query JSON file contents', link: '/howto/query-json' },
            { text: 'Search documents by meaning', link: '/howto/search-by-meaning' },
            { text: 'Add a search index to a table', link: '/howto/search-indexes' },
            { text: "Skip files you don't want indexed", link: '/howto/skip-files' },
            { text: 'Load a SQLite extension', link: '/howto/load-extension' },
            { text: 'Keep the index across restarts', link: '/howto/persist' },
            { text: 'React to file changes', link: '/howto/react-to-changes' },
            { text: 'Write a plugin', link: '/howto/write-a-plugin' },
            { text: 'Embed dirsql in your application', link: '/howto/embed' }
          ]
        },
        {
          text: 'Reference',
          items: [
            { text: 'CLI', link: '/reference/cli' },
            { text: 'Configuration File', link: '/reference/config' },
            { text: 'Command Hooks', link: '/reference/hooks' },
            { text: 'Columns', link: '/reference/columns' },
            { text: 'Path-tables', link: '/reference/path-tables' },
            { text: 'HTTP API', link: '/reference/http-api' },
            { text: 'SDK', link: '/reference/sdk' }
          ]
        },
        {
          text: 'Explanation',
          items: [
            { text: 'How dirsql thinks', link: '/explanation' }
          ]
        },
        {
          text: 'Plugins',
          items: [
            { text: 'Available plugins', link: '/plugins' }
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
