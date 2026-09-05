"""The human-readable diagnosis for a failed npm bundled-cli extension-load probe (#762)."""

from __future__ import annotations

STATIC_MARKER = "Dynamic loading not supported"


def diagnose(result) -> str:
    output = f"stdout={result.stdout!r}\nstderr={result.stderr!r}"
    if STATIC_MARKER in result.stderr:
        return (
            f"the npm bundled-cli `dirsql` binary cannot load SQLite "
            f"extensions ({STATIC_MARKER!r}): it is statically linked "
            "(static-pie), and a static binary cannot dlopen. The release "
            "pipeline (putitoutthere `_matrix.yml`) compiles npm bundle_cli "
            "Linux binaries via cargo-zigbuild against the declared gnu "
            "triple at a pinned glibc floor since putitoutthere#605; a "
            "static binary here means that lane regressed (or the `@v0` "
            "tag resolves to a pre-#605 revision). Fix the pipeline, not "
            "this probe. See dirsql#762.\n"
            f"{output}"
        )
    return f"`dirsql query` against the bundled binary failed.\n{output}"
