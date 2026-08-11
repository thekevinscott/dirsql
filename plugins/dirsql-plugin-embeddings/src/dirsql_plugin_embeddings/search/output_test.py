from . import output


def describe_format_rows():
    def it_formats_each_row_as_path_tab_distance():
        rows = [
            {"path": "notes/planet.txt", "distance": 0.0},
            {"path": "notes/greeting.txt", "distance": 1.0},
        ]
        assert output.format_rows(rows) == [
            "notes/planet.txt\t0.000000",
            "notes/greeting.txt\t1.000000",
        ]

    def it_renders_distances_with_six_decimal_places():
        assert output.format_rows([{"path": "a", "distance": 0.2928932}]) == [
            "a\t0.292893"
        ]

    def it_preserves_row_order():
        rows = [
            {"path": "b", "distance": 0.5},
            {"path": "a", "distance": 0.1},
        ]
        assert [line.split("\t")[0] for line in output.format_rows(rows)] == [
            "b",
            "a",
        ]

    def it_returns_no_lines_for_no_rows():
        assert output.format_rows([]) == []
