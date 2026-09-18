from checks.files.read_config import read_config


def describe_read_config():
    def it_parses_a_toml_config(tmp_path):
        path = tmp_path / "c.toml"
        path.write_text('[[package]]\nname = "x"\nglobs = ["a/**"]\n')
        assert read_config(str(path)) == {"package": [{"name": "x", "globs": ["a/**"]}]}
