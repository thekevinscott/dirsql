"""CLI e2e: a discovered plugin's package-name extension is resolved (#754).

Drives the real launcher (`dirsql.cli.main:main`) + bundled binary with a
plugin *installed* (staged dist on the launcher's `sys.path`) whose
`dirsql.toml` fragment declares a `[[dirsql.extension]]` by bare **package
name**. No mocks. The launcher discovers the plugin, injects the fragment as a
`-c` flag, and must then resolve the fragment's package-name extension to a
literal path (`--extension`) -- the compiled binary cannot resolve package
names itself, so an unresolved name reaches it as a bogus relative path.

The loadable is the repo's `tests/fixtures/testext` cdylib (registers
`dirsql_testext_answer() -> 42`), built on the fly with cargo and staged as an
installed package (`dirsql_testext_pkg`) next to the plugin.
"""

import json
import os
import shutil
import subprocess
import sys

import pytest

_REPO_ROOT = os.path.abspath(
    os.path.join(os.path.dirname(__file__), "..", "..", "..", "..")
)
_FIXTURE_MANIFEST = os.path.join(
    _REPO_ROOT, "packages", "rust", "tests", "fixtures", "testext", "Cargo.toml"
)
_RELEASE = os.path.join(_REPO_ROOT, "target", "release", "dirsql")
_DEBUG = os.path.join(_REPO_ROOT, "target", "debug", "dirsql")
_BINARY = _RELEASE if os.path.exists(_RELEASE) else _DEBUG

# Where the launcher's `binary_path()` looks: `<dirsql package>/_binary/dirsql`.
import dirsql as _dirsql_pkg

_BINARY_STAGE_DIR = os.path.join(os.path.dirname(_dirsql_pkg.__file__), "_binary")

_FRAGMENT = """\
[[dirsql.extension]]
path = "dirsql_testext_pkg"
entrypoint = "sqlite3_extension_init"
"""


def _build_fixture_extension(target_dir):
    proc = subprocess.run(
        [
            os.environ.get("CARGO", "cargo"),
            "build",
            "--manifest-path",
            _FIXTURE_MANIFEST,
            "--target-dir",
            target_dir,
            "--message-format=json",
        ],
        capture_output=True,
        text=True,
    )
    assert proc.returncode == 0, f"fixture build failed:\n{proc.stderr}"
    artifact = None
    for line in proc.stdout.splitlines():
        try:
            msg = json.loads(line)
        except ValueError:
            continue
        if msg.get("reason") == "compiler-artifact":
            for f in msg.get("filenames") or []:
                if f.endswith((".so", ".dylib", ".dll")):
                    artifact = f
    assert artifact, "no cdylib artifact in cargo build output"
    return artifact


def _stage_extension_package(site_dir, so):
    """A real installed package holding the loadable, resolvable by name."""
    pkg = os.path.join(site_dir, "dirsql_testext_pkg")
    os.makedirs(pkg)
    with open(os.path.join(pkg, "__init__.py"), "w") as f:
        f.write("")
    shutil.copy(so, os.path.join(pkg, os.path.basename(so)))


def _stage_plugin(site_dir):
    """Stage a discoverable plugin dist whose fragment names the extension
    package: the importable module plus a `.dist-info` carrying the `dirsql`
    entry point, exactly what `importlib.metadata` scans `sys.path` for."""
    module = os.path.join(site_dir, "dirsql_plugin_ext_fixture")
    os.makedirs(module)
    with open(os.path.join(module, "__init__.py"), "w") as f:
        f.write("")
    with open(os.path.join(module, "dirsql.toml"), "w") as f:
        f.write(_FRAGMENT)
    dist = os.path.join(site_dir, "dirsql_plugin_ext_fixture-0.0.0.dist-info")
    os.makedirs(dist)
    with open(os.path.join(dist, "METADATA"), "w") as f:
        f.write(
            "Metadata-Version: 2.1\nName: dirsql-plugin-ext-fixture\nVersion: 0.0.0\n"
        )
    with open(os.path.join(dist, "entry_points.txt"), "w") as f:
        f.write("[dirsql]\next_fixture = dirsql_plugin_ext_fixture\n")


def _run(site_dir, args, cwd):
    """Run the launcher one-shot with the staged dists on `sys.path`."""
    env = {
        **os.environ,
        "PYTHONPATH": site_dir + os.pathsep + os.environ.get("PYTHONPATH", ""),
    }
    return subprocess.run(
        [
            sys.executable,
            "-c",
            "import sys; from dirsql.cli.main import main; sys.exit(main())",
            *args,
        ],
        cwd=str(cwd),
        capture_output=True,
        text=True,
        env=env,
    )


def describe_plugin_extension_discovery():
    @pytest.fixture
    def staged(tmp_path):
        if shutil.which(os.environ.get("CARGO", "cargo")) is None:
            pytest.skip("cargo not available to build the extension fixture")
        assert os.path.exists(_BINARY), (
            f"dirsql binary not built at {_BINARY}; "
            "run `cargo build -p dirsql --features cli` first"
        )
        so = _build_fixture_extension(str(tmp_path / "target"))

        site_dir = tmp_path / "site"
        site_dir.mkdir()
        _stage_extension_package(str(site_dir), so)
        _stage_plugin(str(site_dir))

        os.makedirs(_BINARY_STAGE_DIR, exist_ok=True)
        staged_binary = os.path.join(_BINARY_STAGE_DIR, "dirsql")
        shutil.copy(_BINARY, staged_binary)
        os.chmod(staged_binary, 0o755)

        data = tmp_path / "data"
        data.mkdir()
        try:
            yield str(site_dir), data
        finally:
            shutil.rmtree(_BINARY_STAGE_DIR, ignore_errors=True)

    def it_resolves_a_discovered_fragments_package_name_extension(staged):
        site_dir, data = staged
        # Pure discovery: no `-c` from the user. The plugin's fragment declares
        # `path = "dirsql_testext_pkg"`; the launcher must resolve it to the
        # staged `.so` or the binary fails to load the extension.
        out = _run(site_dir, ["query", "SELECT dirsql_testext_answer() AS a"], data)
        assert out.returncode == 0, f"stdout={out.stdout!r} stderr={out.stderr!r}"
        assert json.loads(out.stdout) == [{"a": 42}]

    def it_resolves_the_fragment_alongside_a_user_config(staged):
        site_dir, data = staged
        cfg = data / "user.toml"
        cfg.write_text(
            "[[table]]\n"
            'name = "posts"\n'
            'ddl = "CREATE TABLE posts (path TEXT)"\n'
            'glob = "*.md"\n'
            'on-file = "cat {path}"\n'
        )
        # A user `-c` composes with the injected fragment; the fragment's
        # package-name extension must still be resolved.
        out = _run(
            site_dir,
            ["query", "SELECT dirsql_testext_answer() AS a", "-c", "user.toml"],
            data,
        )
        assert out.returncode == 0, f"stdout={out.stdout!r} stderr={out.stderr!r}"
        assert json.loads(out.stdout) == [{"a": 42}]
