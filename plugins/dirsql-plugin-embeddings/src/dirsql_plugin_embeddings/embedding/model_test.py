import sys
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
