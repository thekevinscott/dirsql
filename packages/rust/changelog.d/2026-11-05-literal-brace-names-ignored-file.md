**Fixed** A path glob with no wildcard whose name holds braces bash leaves
literal, such as `./\{q\}.md` or `./{q}.md`, now names a `.gitignore`d
`{q}.md` the way any other literal path does. It used to be treated as a
wildcard glob, so `.gitignore` hid the file.
