from unittest import mock

from checks.wheel_extension_load.list_names import list_names


def describe_list_names():
    def returns_directory_entries():
        listdir = mock.Mock(return_value=["a.whl", "b.txt"])
        assert list_names("dist", listdir) == ["a.whl", "b.txt"]
        listdir.assert_called_once_with("dist")

    def missing_directory_is_empty():
        listdir = mock.Mock(side_effect=FileNotFoundError)
        assert list_names("dist", listdir) == []

    def it_defaults_to_the_real_listdir(tmp_path):
        (tmp_path / "a.whl").write_text("")
        assert list_names(str(tmp_path)) == ["a.whl"]
