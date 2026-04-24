# Migrations

Upgrade guides for `dirsql` consumers. Every release that breaks, removes, or
changes the runtime behavior of a public surface gets its own entry here.

This file is the source of truth. The docs site
([Migrations](https://thekevinscott.github.io/dirsql/migrations)) is generated
from it via a VitePress include; do not edit the rendered page.

See also: [`CHANGELOG.md`](https://github.com/thekevinscott/dirsql/blob/main/CHANGELOG.md) for the full release log. (The relative path is not used because this file is also included into the docs site via a VitePress include, where relative paths would break.)

## [Unreleased]

_No migrations required yet for the next release._

<!--
When a PR introduces a breaking change, a deprecation removal, or a
behavior-only change, copy the template block below into the `## [Unreleased]`
section and fill it in. When a release is cut, rename `## [Unreleased]` to
`## [vX.Y.Z] - YYYY-MM-DD` and start a fresh Unreleased section above it.

Migration entries are required for:
  - Breaking API changes (signatures, names, return types, config keys)
  - Removal of a previously deprecated symbol
  - Behavior changes that keep the same API (exit codes, event payloads,
    on-disk layouts, default values, tag formats)

Migration entries are NOT required for purely additive changes, bug fixes that
restore documented behavior, or changes that are internal-only.
-->

---

## Migration entry template

Copy this block in full. Every subsection is required; if a subsection does
not apply, keep the heading and write `_None._`.

### `<Short title of the change>`

#### Summary

One paragraph. State what broke, which SDKs and call sites are affected, and
why the change was made (bug, parity, redesign, dependency upgrade). A reader
who lands here from a failing build should be able to decide in 30 seconds
whether this migration is the cause.

#### Required changes

A table of before/after snippets covering every affected surface: config
files, CLI flags, action inputs, function signatures, return types. One row
per distinct surface. Include per-SDK snippets where they differ.

| Surface | Before | After |
| ------- | ------ | ----- |
| `<e.g. Python DirSQL.open>` | `<prior call site>` | `<new call site>` |
| `<e.g. CLI flag>` | `<old flag>` | `<new flag>` |

#### Deprecations removed

Anything previously marked deprecated that is now gone. Consumers on the
prior version should have seen warnings; this section tells them which of
those warnings have become hard errors.

- `<deprecated symbol>` (deprecated in `<version>`) — removed; use `<replacement>`.

#### Behavior changes without code changes

Same API, different runtime behavior. Cover exit codes, tag/ID formats,
on-disk layouts, event payloads, retry behavior, default values. Each bullet
names the surface and describes the old vs. new behavior concretely.

- `<surface>`: previously `<old behavior>`; now `<new behavior>`. `<impact on
  consumer code, if any>`.

#### Verification

A concrete recipe a consumer can run to confirm the upgrade worked. Prefer a
dry-run or read-only command plus expected output; do not require them to
mutate real data.

```bash
<command>
# expected: <output>
```
