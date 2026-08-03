**Fixed** The manylinux wheel's bundled `dirsql` binary is now dynamically
linked against glibc instead of static-pie musl, so SQLite extension loading
(`[[dirsql.extension]]`, the embeddings plugin, `sqlite-vec`) works from the
published PyPI artifact. A static-pie binary cannot `dlopen`, so every
extension load failed with `Dynamic loading not supported` on Linux via
`pip`/`uvx`. The linking change lands in the release pipeline
(putitoutthere's `_matrix.yml` pypi `bundle_cli` lane); a PR-time
extension-load probe in `release-precheck.yml` guards against regression.
(#755)
