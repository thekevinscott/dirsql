"""Unit tests for `dispatch_extract`."""

from types import SimpleNamespace

from dirsql.cli.interpret.dispatch_extract import dispatch_extract


def _table(extract):
    return SimpleNamespace(extract=extract)


def describe_dispatch_extract():
    def it_returns_ok_true_with_the_rows_extract_produced():
        tables = {"papers": _table(lambda p: [{"title": p}])}
        out = dispatch_extract(
            {"type": "extract", "id": 1, "table": "papers", "path": "/x.json"},
            tables,
        )
        assert out == {
            "type": "result",
            "id": 1,
            "ok": True,
            "rows": [{"title": "/x.json"}],
        }

    def it_echoes_the_request_id_verbatim_on_success():
        tables = {"papers": _table(lambda _p: [])}
        out = dispatch_extract(
            {"type": "extract", "id": 99, "table": "papers", "path": "/x.json"},
            tables,
        )
        assert out["id"] == 99

    def it_returns_ok_false_with_the_table_name_when_the_table_is_unknown():
        out = dispatch_extract(
            {"type": "extract", "id": 5, "table": "ghost", "path": "/x"},
            tables={"papers": _table(lambda _p: [])},
        )
        assert out["type"] == "result"
        assert out["id"] == 5
        assert out["ok"] is False
        assert "ghost" in out["error"]

    def it_echoes_the_request_id_verbatim_on_unknown_table():
        out = dispatch_extract(
            {"type": "extract", "id": 17, "table": "ghost", "path": "/x"},
            tables={},
        )
        assert out["id"] == 17

    def it_returns_ok_false_with_the_exception_message_when_extract_raises():
        def boom(_p):
            raise ValueError("synthetic")

        out = dispatch_extract(
            {"type": "extract", "id": 7, "table": "papers", "path": "/x"},
            {"papers": _table(boom)},
        )
        assert out == {
            "type": "result",
            "id": 7,
            "ok": False,
            "error": "synthetic",
        }

    def it_passes_the_request_path_to_the_extract_callback():
        seen: list[str] = []

        def capture(p):
            seen.append(p)
            return []

        dispatch_extract(
            {"type": "extract", "id": 1, "table": "t", "path": "/abs/x.json"},
            {"t": _table(capture)},
        )
        assert seen == ["/abs/x.json"]

    def it_treats_a_missing_table_field_as_unknown_table():
        out = dispatch_extract(
            {"type": "extract", "id": 1, "path": "/x"},
            tables={"t": _table(lambda _p: [])},
        )
        assert out["ok"] is False
        assert "None" in out["error"] or "none" in out["error"].lower()

    def it_passes_through_a_missing_id_field_as_none():
        out = dispatch_extract(
            {"type": "extract", "table": "ghost", "path": "/x"},
            tables={},
        )
        assert out["id"] is None
