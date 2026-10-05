from unittest import mock

from checks.npm_addon_version.committed_version import committed_version


def describe_committed_version():
    def reads_the_package_version():
        config = mock.Mock(return_value={"package": {"version": "0.2.7"}})
        assert committed_version("Cargo.toml", config) == "0.2.7"
        config.assert_called_once_with("Cargo.toml")

    def it_defaults_to_the_real_toml_reader(tmp_path):
        manifest = tmp_path / "Cargo.toml"
        manifest.write_text('[package]\nname = "x"\nversion = "1.2.3"\n')
        assert committed_version(str(manifest)) == "1.2.3"
