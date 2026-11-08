**Fixed** A config `[[table]] glob` written `.//a/*.json` (any run of
separators after `./`) now matches the same files as `./a/*.json`, as in bash.
It used to keep a leading `/` and match nothing.
