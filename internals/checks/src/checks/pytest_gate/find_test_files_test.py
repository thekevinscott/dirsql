from checks.pytest_gate.find_test_files import find_test_files


def describe_find_test_files():
    def finds_nested_test_files_recursively(tmp_path):
        nested = tmp_path / "a" / "b"
        nested.mkdir(parents=True)
        target = nested / "check_test.py"
        target.write_text("")
        (nested / "check.py").write_text("")
        assert find_test_files([str(tmp_path)]) == [target]

    def returns_empty_when_no_paths():
        assert find_test_files([]) == []
