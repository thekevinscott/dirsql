**Changed** A declared `[[table]] glob` that names a directory, `glob = "docs"`
or `glob = "docs/"`, lists the files directly inside it, like `FROM './docs'`.
It used to match nothing.
