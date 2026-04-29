# Migrations

Upgrade guides for `dirsql` consumers. Every release that breaks, removes, or
changes the runtime behavior of a public surface gets its own entry here.

This file is the source of truth. The docs site
([Migrations](https://thekevinscott.github.io/dirsql/migrations)) is generated
from it via a VitePress include; do not edit the rendered page.

See also: [`CHANGELOG.md`](https://github.com/thekevinscott/dirsql/blob/main/CHANGELOG.md) for the full release log. (The relative path is not used because this file is also included into the docs site via a VitePress include, where relative paths would break.)

## [Unreleased]

### Release pipeline migrated to `putitoutthere`

#### Summary

The release process is now driven by [putitoutthere](https://github.com/thekevinscott/putitoutthere). No SDK call sites change; the migration is observable in tag layout, npm package layout, and CI configuration. Consumers installing via `pip install dirsql` / `cargo add dirsql` / `npm install dirsql` see no behavioral difference at install time. Operators reading release tags or pinning npm sub-packages by name need to update their references.

#### Required changes

| Surface | Before | After |
|---|---|---|
| Git tag for a release | one shared tag `v{version}` | three per-package tags `dirsql-rust-v{version}`, `dirsql-py-v{version}`, `dirsql-npm-v{version}` |
| npm CLI sub-packages | `@dirsql/cli-<short-slug>` (e.g. `@dirsql/cli-linux-x64-gnu`) | `@dirsql/cli-{triple}` (e.g. `@dirsql/cli-linux-x64-gnu`) — same scheme, retained via `name` template |
| npm napi sub-packages | `@dirsql/lib-<short-slug>` | `@dirsql/lib-{triple}` — same scheme, retained via `name` template |
| Release trigger | scheduled cron + immediate-on-push (toggle via `RELEASE_STRATEGY` repo var) | every push to `main` whose changes match a package's `globs` |
| Skip a release | `[no-release]` in commit message | `release: skip` trailer in commit body |
| Bump type | `workflow_dispatch` input (`patch` / `minor`) | `release: <bump>` trailer in commit body (default `patch`) |
| Publish auth | bootstrap `NPM_TOKEN` + `crates-io-auth-action` + PyPI TP | OIDC trusted publishers on all three registries; no long-lived tokens reachable from the workflow |

#### Deprecations removed

_None._

#### Behavior changes without code changes

- **Per-SDK selective publishing.** The `workflow_dispatch` `publish_python` / `publish_rust` / `publish_js` toggles are gone; package selection now flows through `release: <bump> [<pkg-name>, ...]` trailers (per-package names: `dirsql-rust`, `dirsql-py`, `dirsql-npm`).
- **Auto-rollback on partial publish failure** is no longer performed. The previous pipeline deleted the tag if both PyPI and crates.io publishes failed; under putitoutthere, a partial failure leaves the published artifacts in place and re-runs are idempotent (each handler's first move is `isPublished`, which short-circuits cleanly on already-published versions).
- **GitHub Release notes** are still auto-generated (`gh release create --generate-notes`) but the Release is now created by the reusable workflow, not the consumer's `publish.yml`.
- **Dry-run mode** is removed. The plan job is side-effect-free; inspect the matrix output on a feature branch to preview a release.

#### Verification

```
# 1. The new caller workflow lints clean.
yamllint .github/workflows/release.yml

# 2. The toml parses and the plan resolves.
#    (Locally — putitoutthere's `plan` is pure over (config + git state).)
npx -y putitoutthere@0.2 plan

# 3. Trusted publishers on all three registries point at this filename.
#    Expected entry on each:
#      Repository: thekevinscott/dirsql
#      Workflow:   release.yml
#      Environment: release
#    PyPI:    https://pypi.org/manage/project/dirsql/settings/publishing/
#    crates:  https://crates.io/crates/dirsql/settings
#    npm:     https://www.npmjs.com/package/dirsql/access
#             — plus one per per-platform package (see PR body).
```

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
