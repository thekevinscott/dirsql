"""Tests for compute_version.

Determines the next release version given the latest git tag and the bump
type. This used to live in the `tag` job of publish.yml as inline shell;
the derivation bug that published crates.io 0.1.0 twice (rollback deleted
the tag, next run recomputed 0.1.0 off 0.0.0) lived here.
"""

import pytest

from compute_version import Decision, compute


def describe_patch_bumps():
    def from_existing_tag():
        assert compute(latest_tag="v0.1.0", bump_type="patch", commits_since_tag=3) == (
            Decision(new_version="0.1.1", should_release=True)
        )

    def increments_two_digit_patch():
        result = compute(latest_tag="v0.1.9", bump_type="patch", commits_since_tag=1)
        assert result.new_version == "0.1.10"

    def no_commits_since_tag_skips_release():
        # Rationale: scheduled patch releases fire daily; if nothing landed
        # since the last tag, skip rather than cut an empty release.
        result = compute(latest_tag="v0.1.0", bump_type="patch", commits_since_tag=0)
        assert result.should_release is False

    def first_release_has_no_tag():
        # commits_since_tag is meaningless without a tag; should_release is
        # always true.
        result = compute(latest_tag="", bump_type="patch", commits_since_tag=0)
        assert result == Decision(new_version="0.0.1", should_release=True)


def describe_minor_bumps():
    def resets_patch_to_zero():
        result = compute(latest_tag="v0.1.7", bump_type="minor", commits_since_tag=5)
        assert result.new_version == "0.2.0"

    def zero_commits_still_releases():
        # Minor releases are always manual; the operator chose to cut one.
        # Don't second-guess them like patch does.
        result = compute(latest_tag="v0.1.0", bump_type="minor", commits_since_tag=0)
        assert result.should_release is True

    def first_minor_release_with_no_tag():
        result = compute(latest_tag="", bump_type="minor", commits_since_tag=0)
        assert result == Decision(new_version="0.1.0", should_release=True)


def describe_validation():
    def rejects_malformed_tag():
        with pytest.raises(ValueError, match="v1.2"):
            compute(latest_tag="v1.2", bump_type="patch", commits_since_tag=1)

    def rejects_unknown_bump_type():
        with pytest.raises(ValueError, match="major"):
            compute(latest_tag="v0.1.0", bump_type="major", commits_since_tag=1)

    def rejects_negative_commits():
        with pytest.raises(ValueError):
            compute(latest_tag="v0.1.0", bump_type="patch", commits_since_tag=-1)
