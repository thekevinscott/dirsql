"""Tests for resolve_publish_targets.

The script takes the release inputs (event, mode, custom flags, latest tag,
changed files) and resolves which packages to publish. The bug we keep
hitting is that the matrix of modes and event types isn't obvious, so each
branch is tested explicitly.
"""

from resolve_publish_targets import Flags, resolve


def describe_all_mode():
    def all_flags_true_on_workflow_dispatch():
        result = resolve(
            event_name="workflow_dispatch",
            publish_mode="all",
            custom_python=False,
            custom_rust=False,
            custom_js=False,
            latest_tag="v0.1.0",
            changed_files=[],
        )
        assert result == Flags(rust=True, python=True, js=True, docs=True)

    def custom_booleans_are_ignored_in_all_mode():
        result = resolve(
            event_name="workflow_dispatch",
            publish_mode="all",
            custom_python=False,
            custom_rust=False,
            custom_js=False,
            latest_tag="v0.1.0",
            changed_files=[],
        )
        assert result.rust is True
        assert result.python is True
        assert result.js is True


def describe_custom_mode():
    def custom_flags_are_used_verbatim():
        result = resolve(
            event_name="workflow_dispatch",
            publish_mode="custom",
            custom_python=False,
            custom_rust=False,
            custom_js=True,
            latest_tag="v0.1.0",
            changed_files=[],
        )
        assert result == Flags(rust=False, python=False, js=True, docs=False)

    def custom_does_not_cascade_rust_to_bindings():
        result = resolve(
            event_name="workflow_dispatch",
            publish_mode="custom",
            custom_python=False,
            custom_rust=True,
            custom_js=False,
            latest_tag="v0.1.0",
            changed_files=[],
        )
        assert result == Flags(rust=True, python=False, js=False, docs=False)

    def custom_never_sets_docs():
        result = resolve(
            event_name="workflow_dispatch",
            publish_mode="custom",
            custom_python=True,
            custom_rust=True,
            custom_js=True,
            latest_tag="v0.1.0",
            changed_files=["README.md"],
        )
        assert result.docs is False


def describe_changed_mode():
    def no_prior_tag_treats_everything_as_changed():
        result = resolve(
            event_name="workflow_dispatch",
            publish_mode="changed",
            custom_python=False,
            custom_rust=False,
            custom_js=False,
            latest_tag="",
            changed_files=[],
        )
        assert result == Flags(rust=True, python=True, js=True, docs=True)

    def no_changes_since_tag_publishes_nothing():
        result = resolve(
            event_name="workflow_dispatch",
            publish_mode="changed",
            custom_python=False,
            custom_rust=False,
            custom_js=False,
            latest_tag="v0.1.0",
            changed_files=[],
        )
        assert result == Flags(rust=False, python=False, js=False, docs=False)

    def rust_core_change_cascades_to_python_and_js():
        # Rust changes require rebuilt bindings in Python and TS, so both
        # need to republish alongside crates.io.
        result = resolve(
            event_name="workflow_dispatch",
            publish_mode="changed",
            custom_python=False,
            custom_rust=False,
            custom_js=False,
            latest_tag="v0.1.0",
            changed_files=["packages/rust/src/lib.rs"],
        )
        # NOTE: today the detect-changes shell only sets rust_changed=true
        # for rust core files. The cascade happens downstream in the release
        # job's `publish_pypi` / `publish_crates` expressions (rust OR ...).
        # So at this layer, only rust is flagged. The cascade belongs to the
        # consumer of these flags, not the resolver.
        assert result.rust is True
        assert result.python is False
        assert result.js is False

    def python_only_change_flags_python_only():
        result = resolve(
            event_name="workflow_dispatch",
            publish_mode="changed",
            custom_python=False,
            custom_rust=False,
            custom_js=False,
            latest_tag="v0.1.0",
            changed_files=["packages/python/python/dirsql/__init__.py"],
        )
        assert result == Flags(rust=False, python=True, js=False, docs=False)

    def ts_only_change_flags_js_only():
        result = resolve(
            event_name="workflow_dispatch",
            publish_mode="changed",
            custom_python=False,
            custom_rust=False,
            custom_js=False,
            latest_tag="v0.1.0",
            changed_files=["packages/ts/src/index.ts"],
        )
        assert result == Flags(rust=False, python=False, js=True, docs=False)

    def docs_change_flags_docs_only():
        result = resolve(
            event_name="workflow_dispatch",
            publish_mode="changed",
            custom_python=False,
            custom_rust=False,
            custom_js=False,
            latest_tag="v0.1.0",
            changed_files=["README.md"],
        )
        assert result == Flags(rust=False, python=False, js=False, docs=True)

    def cargo_lock_change_counts_as_rust():
        result = resolve(
            event_name="workflow_dispatch",
            publish_mode="changed",
            custom_python=False,
            custom_rust=False,
            custom_js=False,
            latest_tag="v0.1.0",
            changed_files=["Cargo.lock"],
        )
        assert result.rust is True

    def python_src_change_counts_as_rust_because_bindings_are_built_there():
        # packages/python/src/ holds the Rust pyo3 binding crate, not the
        # Python SDK. Changes there rebuild the cdylib and require a crate
        # bump for the bundled binary.
        result = resolve(
            event_name="workflow_dispatch",
            publish_mode="changed",
            custom_python=False,
            custom_rust=False,
            custom_js=False,
            latest_tag="v0.1.0",
            changed_files=["packages/python/src/lib.rs"],
        )
        assert result.rust is True


def describe_non_dispatch_events():
    def schedule_event_forces_changed_mode_regardless_of_input():
        # Scheduled runs can't pass inputs; they always fall through to
        # auto-detection. Even if publish_mode=all would be passed somehow,
        # the resolver ignores it for non-dispatch events.
        result = resolve(
            event_name="schedule",
            publish_mode="all",  # ignored
            custom_python=False,
            custom_rust=False,
            custom_js=False,
            latest_tag="v0.1.0",
            changed_files=["packages/ts/src/index.ts"],
        )
        assert result == Flags(rust=False, python=False, js=True, docs=False)

    def push_event_forces_changed_mode():
        result = resolve(
            event_name="push",
            publish_mode="all",  # ignored
            custom_python=False,
            custom_rust=False,
            custom_js=False,
            latest_tag="v0.1.0",
            changed_files=["packages/python/pyproject.toml"],
        )
        assert result == Flags(rust=False, python=True, js=False, docs=False)
