from checks.npm_binary_extension_load.config_for import ENTRYPOINT, config_for


def describe_entrypoint():
    def it_names_sqlite_vecs_init_symbol():
        assert ENTRYPOINT == "sqlite3_vec_init"


def describe_config_for():
    def declares_the_library_and_entrypoint():
        assert config_for("/v/vec0") == (
            '[[dirsql.extension]]\npath = "/v/vec0"\n'
            'entrypoint = "sqlite3_vec_init"\n'
        )

    def it_interpolates_the_library_path():
        assert 'path = "/other/lib"\n' in config_for("/other/lib")
