**Changed** A declared `[[table]] glob` no longer matches a dot-named file or
directory unless the glob spells it: `**/*.md` skips `.env.md` and
`.hid/z.md`, while `.hid/*.md` lists `.hid/z.md`.
