**Added**

`dirsql::extension_resolution` plans a set of configs' `[[dirsql.extension]]`
entries: it parses the TOML, decides whether any entry names a package rather
than a file, resolves the literal entries against each config's own directory,
and picks a package's loadable file out of a listing the host supplies. The
Python and TypeScript launchers each carry a port of this today; they migrate
onto it next.
