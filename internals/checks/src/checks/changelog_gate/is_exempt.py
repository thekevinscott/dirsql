"""Tell a package's source files from its bookkeeping (#494/#496)."""

import re


def is_exempt(path: str, pkg: str) -> bool:
    """True if ``path`` is a fragment, stub, or test file -- not source.

    The package ``CHANGELOG.md`` / ``MIGRATIONS.md`` are pointer stubs; the
    ``changelog.d/`` / ``migrations.d/`` folders are the entries themselves;
    the ``e2e-attestations/`` folder holds CI freshness receipts, not source;
    a package-root ``testing-conventions.toml`` is gate config that ships in no
    published artifact, so nothing user-facing can follow from editing one;
    dirsql colocates Python unit tests as ``*_test.py`` and TS as ``*.test.*``
    / ``*.spec.*``; and anything under a ``tests/`` directory is a test tier.
    """
    p = re.escape(pkg)
    return bool(
        re.fullmatch(rf"{p}/(CHANGELOG|MIGRATIONS)\.md", path)
        or re.match(rf"{p}/(changelog|migrations)\.d/", path)
        or re.match(rf"{p}/e2e-attestations/", path)
        or re.fullmatch(rf"{p}/testing-conventions\.toml", path)
        or re.fullmatch(rf"{p}/.*_test\.py", path)
        or re.fullmatch(rf"{p}/.*\.(test|spec)\.(ts|tsx|js|mjs|cjs)", path)
        or re.match(rf"{p}/tests?/", path)
    )
