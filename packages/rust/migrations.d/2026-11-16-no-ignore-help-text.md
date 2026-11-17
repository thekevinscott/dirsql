### `--no-ignore` help text matches path-table skip rules

#### Summary

The `--no-ignore` help text now states that only the built-in `.git` skip applies to path-tables and that a config's `ignore` never does. Help text only; behavior is unchanged and nothing breaks.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

_None._

#### Verification

`dirsql --help` describes `--no-ignore` with "The built-in `.git` skip still applies; a config's `ignore` never applies to path-tables."
