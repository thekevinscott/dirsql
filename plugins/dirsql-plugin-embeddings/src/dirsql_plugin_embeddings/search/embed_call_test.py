from unittest.mock import patch

from . import embed_call as module


def call(argument, model):
    with patch.object(module, "quote", side_effect=lambda text: f"<{text}>"):
        return module.embed_call(argument, model)


def describe_embed_call():
    def it_omits_the_model_argument_when_model_is_none():
        assert call("content", None) == "embed(content)"

    def it_quotes_the_model_id_as_the_second_argument():
        assert call("content", "my/model") == "embed(content, <my/model>)"
