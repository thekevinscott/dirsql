# Changelog and Migrations — full mechanics

Extracted from AGENTS.md (see "Changelog and Migrations" there for the summary).


**Every PR that touches public-facing SDK code must add a changelog fragment.** This is enforced in CI by the `changelog-gate` check (`internals/checks`), whose implementation mirrors [template-lib](https://github.com/thekevinbot/template-lib)'s reference gate (#566); an unmet gate blocks merge.

The scope: any change to non-test source under a package root -- `packages/<pkg>/` (the three SDKs) or `plugins/<pkg>/` (independently published plugins, #896) -- requires a fragment naming that package. A package is identified by its root-qualified directory, so `plugins/ts` and `packages/ts` would be two packages. Exempt are test files (`*_test.py`, `*.test.ts` / `*.spec.ts`, anything under `<root>/<pkg>/tests/`), the package `CHANGELOG.md` / `MIGRATIONS.md` pointer stubs, the `e2e-attestations/` receipts, and the fragment folders themselves.

**A package `README.md` is source, not docs, and does require a fragment** -- `_is_exempt` in `changelog_gate/decide.py` deliberately omits it. READMEs ship inside all three published artifacts (the PyPI long-description, the npm tarball, the crate), so an edit reaches users the way code does; `release-ci.yml` excludes the fragment dirs and attestation receipts from its triggers but pointedly keeps READMEs, for the same reason. So a README-only PR runs both `release-ci` and `changelog-check`, and needs a fragment (or a `skip-changelog:` reason). This is the one place where the "docs are free" rule of #834's path-filter spec does not apply -- that epic's table listed `README.md` alongside the root prose files, and the maintainer decision during its final review was to keep the gate as written and correct the spec instead. We err toward requiring entries because the project does not yet strictly follow semver, so the changelog must carry the signal that semver would otherwise provide.

**Fragments are per-package and colocated (#565), so they ship with the package.** Each SDK package (`python`, `ts`, `rust`) owns its own changelog under `packages/<pkg>/changelog.d/`, as does each plugin under `plugins/<pkg>/changelog.d/`, and a PR adds one fragment per **changed package** -- the fragment lives under the same package whose source changed:

```
<root>/<pkg>/changelog.d/YYYY-MM-DD-<slug>.md
```

- `<pkg>` is the package whose public source the PR changed. The Rust core is `rust` (`packages/rust/`), the Python package/binding is `python` (`packages/python/`), the TS package + napi crate is `ts` (`packages/ts/`), and the embeddings plugin is `plugins/dirsql-plugin-embeddings/`. The directory identifies the package, so the filename carries no package token. A PR that touches more than one package needs a fragment in each.
- `YYYY-MM-DD` is the UTC merge date; `<slug>` is a short kebab-case description (`2026-07-13-fix-watcher-race.md`).
- The body leads with a Keep a Changelog **category** in bold -- `**Added**` / `**Changed**` / `**Deprecated**` / `**Removed**` / `**Fixed**` / `**Security**` -- then the entry text, exactly as it would read in a changelog. The category lives in the body, **not** the filename.

Fragments are **permanent and append-only** -- nothing is ever assembled back into a root `CHANGELOG.md` and deleted. The root `CHANGELOG.md` / `MIGRATIONS.md` are **frozen** pointer stubs holding only the pre-fragment history (#563/#564); do not edit them. Version history is the `git log --tags` record (the repo tags a release on every merge).

> **Direction of travel is one-way: entries become fragments, never the reverse.** The root `CHANGELOG.md` / `MIGRATIONS.md` are a *closed archive* -- a new entry (even one that documents an already-released change, or a stray fragment left in an old location) is **never** appended, merged, or "folded" into them. The correct home for *any* changelog/migration content that is not already frozen is a fragment under `<root>/<pkg>/changelog.d/` (or `migrations.d/`). If you find loose entries in a wrong location -- e.g. the retired **root** `changelog.d/` / `migrations.d/` (the dual-mode dirs that predate the per-package layout, #565) -- **relocate them to the owning package's fragment dir** (renamed to `YYYY-MM-DD-<slug>.md`, body leading with its category), one copy per package the change affected; do **not** move them into the frozen files. Writing into the frozen archive is the mistake the freeze exists to prevent -- if you're adding lines to root `CHANGELOG.md`/`MIGRATIONS.md`, stop: you want a fragment.

**Escape hatch.** If a PR genuinely has no observable change -- a pure refactor, an internal rename, a type-signature tidy with the same runtime -- bypass the gate by adding a `skip-changelog:` line to any commit message on the PR:

```
skip-changelog: <reason>
```

The gate scans raw commit bodies (#566, mirroring template-lib), so the line works from **any** line of any commit -- it need not be a formal git trailer, which removes the blank-line-splits-the-trailer footgun entirely. The reason stays in git history, so the decision is auditable. Use this sparingly; when in doubt, write the changelog fragment.

**A migration fragment is additionally required when a PR:**

- Breaks a public API (signature, name, return type, config key, CLI flag, action input).
- Removes a previously deprecated symbol.
- Changes runtime behavior without changing the API (exit codes, event payloads, on-disk layouts, default values, tag formats).

Purely additive changes and behavior-preserving bug fixes do NOT require a migration entry.

Migration fragments are per-package too, one file per changed package under `<root>/<pkg>/migrations.d/YYYY-MM-DD-<slug>.md` (same naming as changelog fragments). Each is a complete entry -- a `### <title>` heading plus the five required subsections:

1. **Summary** -- one paragraph: what broke, which SDKs/call sites, and why.
2. **Required changes** -- table of before/after snippets for every affected surface (config, CLI, action inputs, function signatures, return types).
3. **Deprecations removed** -- previously warned symbols that are now hard errors.
4. **Behavior changes without code changes** -- same API, different runtime behavior.
5. **Verification** -- a concrete dry-run command plus expected output that a consumer can run to confirm the upgrade.

If a subsection does not apply, keep the heading and write `_None._`. Do not omit subsections. The template lives at the bottom of the frozen root `MIGRATIONS.md`.

The frozen root `MIGRATIONS.md` is not published on the docs site: it holds only pre-fragment history, so a page built from it silently omits every migration written under the fragment convention -- an upgrade guide that looks authoritative and is not (#885). Aggregating `packages/*/migrations.d/*.md` into a page at build time is the option not taken; it needs a build step and a fragment ordering convention.

**PR body requirement:** PRs that touch SDK code must contain the following block (checkboxes filled in):

```markdown
## Changelog / Migrations

- [ ] Changelog fragment added under `<root>/<pkg>/changelog.d/` for each changed package (or: `skip-changelog` trailer on a commit with reason)
- [ ] Migration fragment added under `<root>/<pkg>/migrations.d/` (or: not required -- additive/bugfix only)
```

Orchestrators must block merges of SDK-touching PRs that miss either file when required.
