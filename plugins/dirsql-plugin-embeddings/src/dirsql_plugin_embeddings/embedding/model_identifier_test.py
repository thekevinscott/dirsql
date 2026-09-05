from types import SimpleNamespace

from . import model_identifier as module


def describe_model_identifier():
    def it_appends_the_model2vec_version_when_the_config_carries_one():
        loaded = SimpleNamespace(config={"model2vec_version": "0.3.0"})
        assert (
            module.model_identifier("some/model-id", loaded)
            == "some/model-id@0.3.0"
        )

    def it_is_the_model_id_alone_when_the_config_has_no_version():
        loaded = SimpleNamespace(config={"normalize": False})
        assert module.model_identifier("some/model-id", loaded) == "some/model-id"

    def it_is_the_model_id_alone_when_the_config_is_none():
        loaded = SimpleNamespace(config=None)
        assert module.model_identifier("some/model-id", loaded) == "some/model-id"

    def it_is_the_model_id_alone_when_there_is_no_config_attribute():
        assert module.model_identifier("some/model-id", object()) == "some/model-id"

    def it_treats_an_empty_version_as_absent():
        loaded = SimpleNamespace(config={"model2vec_version": ""})
        assert module.model_identifier("some/model-id", loaded) == "some/model-id"
