"""Tests for check_published.

Registry-idempotency probes. Each registry has a different 404 vs. 200
contract and a different response shape, so each is tested explicitly.

HTTP is injected as a callable to keep tests hermetic -- no real network.
"""

import json

import pytest

from check_published import HttpResponse, check_published


def _fake(responses: dict[str, HttpResponse]):
    """Build an http_get that looks up responses by exact URL match."""

    def http_get(url: str) -> HttpResponse:
        if url not in responses:
            raise AssertionError(f"unexpected URL: {url}")
        return responses[url]

    return http_get


def describe_crates_io():
    def returns_true_when_version_is_in_list():
        body = json.dumps(
            {"crate": {"max_version": "0.1.0"}, "versions": [{"num": "0.0.26"}, {"num": "0.1.0"}]}
        )
        http = _fake({"https://crates.io/api/v1/crates/dirsql": HttpResponse(200, body)})
        assert check_published(registry="crates", package="dirsql", version="0.1.0", http_get=http) is True

    def returns_false_when_version_not_in_list():
        body = json.dumps({"versions": [{"num": "0.0.26"}]})
        http = _fake({"https://crates.io/api/v1/crates/dirsql": HttpResponse(200, body)})
        assert check_published(registry="crates", package="dirsql", version="0.1.0", http_get=http) is False

    def returns_false_when_crate_does_not_exist():
        http = _fake({"https://crates.io/api/v1/crates/dirsql": HttpResponse(404, "")})
        assert check_published(registry="crates", package="dirsql", version="0.1.0", http_get=http) is False


def describe_pypi():
    def returns_true_when_version_endpoint_returns_200():
        http = _fake(
            {"https://pypi.org/pypi/dirsql/0.1.0/json": HttpResponse(200, '{"info":{"version":"0.1.0"}}')}
        )
        assert check_published(registry="pypi", package="dirsql", version="0.1.0", http_get=http) is True

    def returns_false_when_version_endpoint_returns_404():
        http = _fake({"https://pypi.org/pypi/dirsql/0.2.0/json": HttpResponse(404, "")})
        assert check_published(registry="pypi", package="dirsql", version="0.2.0", http_get=http) is False


def describe_npm():
    def returns_true_when_version_endpoint_returns_200():
        http = _fake(
            {"https://registry.npmjs.org/dirsql/0.1.0": HttpResponse(200, '{"version":"0.1.0"}')}
        )
        assert check_published(registry="npm", package="dirsql", version="0.1.0", http_get=http) is True

    def returns_false_when_version_endpoint_returns_404():
        http = _fake({"https://registry.npmjs.org/dirsql/0.2.0": HttpResponse(404, "")})
        assert check_published(registry="npm", package="dirsql", version="0.2.0", http_get=http) is False

    def url_encodes_scoped_packages():
        # @scope/name -> %40scope%2Fname on npm
        http = _fake(
            {"https://registry.npmjs.org/%40thekevinscott%2Fdirsql/0.1.0": HttpResponse(200, "{}")}
        )
        assert (
            check_published(
                registry="npm",
                package="@thekevinscott/dirsql",
                version="0.1.0",
                http_get=http,
            )
            is True
        )


def describe_errors():
    def raises_on_unknown_registry():
        with pytest.raises(ValueError, match="unknown registry"):
            check_published(registry="gem", package="dirsql", version="0.1.0", http_get=_fake({}))

    def raises_on_unexpected_status():
        # 5xx from a registry is a transient failure -- do NOT treat as
        # "not published" (that would cause a duplicate publish attempt).
        http = _fake({"https://crates.io/api/v1/crates/dirsql": HttpResponse(503, "")})
        with pytest.raises(RuntimeError, match="503"):
            check_published(registry="crates", package="dirsql", version="0.1.0", http_get=http)
