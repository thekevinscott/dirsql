**Fixed** An `on-file` command whose program cannot be spawned now names the
program rather than the whole unsplit template. A program named without a `/`
that is missing from `$PATH` says so (`not found on $PATH`), and when a file of
that name sits in the command's working directory the error offers the `./`
form. Resolution is unchanged: a bare name is never run from the scanned
directory. `CommandError::Spawn` carries `program` instead of `command`, and
the new `CommandError::NotOnPath` covers the `$PATH` case.
