from unittest.mock import patch

from . import count_corpus_sql as module


def count(glob):
    with patch.object(module, "quote", side_effect=lambda text: f"<{text}>"):
        with patch.object(
            module, "normalize_glob", side_effect=lambda g: f"N({g})"
        ) as normalize:
            return module.count_corpus_sql(glob), normalize


def describe_count_corpus_sql():
    def it_counts_the_files_the_normalized_quoted_glob_matches():
        sql, normalize = count("docs/**/*.md")
        assert sql == "SELECT COUNT(*) AS n FROM <N(docs/**/*.md)>"
        normalize.assert_called_once_with("docs/**/*.md")
