from unittest.mock import AsyncMock, patch

import pytest

from . import run


def describe_run_search():
    def it_formats_the_rows_the_search_returned():
        rows = [{"path": "a", "distance": 0.5}]
        found = AsyncMock(return_value=(rows, None))
        with patch.object(run, "search_rows", found):
            with patch.object(
                run, "format_rows", return_value=["line"]
            ) as formatter:
                assert run.run_search("g", "q", 5, "m") == ["line"]
        found.assert_awaited_once_with("g", "q", 5, "m")
        formatter.assert_called_once_with(rows)

    def it_defaults_the_model_to_none():
        found = AsyncMock(return_value=([{"path": "a"}], None))
        with patch.object(run, "search_rows", found):
            with patch.object(run, "format_rows", return_value=[]):
                run.run_search("g", "q", 5)
        found.assert_awaited_once_with("g", "q", 5, None)

    def it_raises_with_the_no_rows_message_when_nothing_ranked():
        with patch.object(run, "search_rows", AsyncMock(return_value=([], 3))):
            with patch.object(
                run, "no_rows_message", return_value="why"
            ) as message:
                with patch("os.getcwd", return_value="/work"):
                    with pytest.raises(run.NothingToRank, match="why"):
                        run.run_search("g", "q", 5)
        message.assert_called_once_with("g", 3, "/work")
