import sys
from types import SimpleNamespace
from unittest.mock import MagicMock, patch

from . import model


def describe_default_model_id():
    def it_is_potion_retrieval_32m():
        assert model.DEFAULT_MODEL_ID == "minishlab/potion-retrieval-32M"


def describe_load_model():
    def it_loads_via_model2vec_from_pretrained():
        model2vec = MagicMock()
        with patch.dict(sys.modules, {"model2vec": model2vec}):
            loaded = model.load_model("some/model-id")
        model2vec.StaticModel.from_pretrained.assert_called_once_with(
            "some/model-id"
        )
        assert loaded is model2vec.StaticModel.from_pretrained.return_value


def describe_model_identifier():
    def it_appends_the_model2vec_version_when_the_config_carries_one():
        loaded = SimpleNamespace(config={"model2vec_version": "0.3.0"})
        assert (
            model.model_identifier("some/model-id", loaded)
            == "some/model-id@0.3.0"
        )

    def it_is_the_model_id_alone_when_the_config_has_no_version():
        loaded = SimpleNamespace(config={"normalize": False})
        assert model.model_identifier("some/model-id", loaded) == "some/model-id"

    def it_is_the_model_id_alone_when_the_config_is_none():
        loaded = SimpleNamespace(config=None)
        assert model.model_identifier("some/model-id", loaded) == "some/model-id"

    def it_is_the_model_id_alone_when_there_is_no_config_attribute():
        assert model.model_identifier("some/model-id", object()) == "some/model-id"

    def it_treats_an_empty_version_as_absent():
        loaded = SimpleNamespace(config={"model2vec_version": ""})
        assert model.model_identifier("some/model-id", loaded) == "some/model-id"
