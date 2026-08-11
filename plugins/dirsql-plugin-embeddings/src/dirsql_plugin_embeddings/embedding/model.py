DEFAULT_MODEL_ID = "minishlab/potion-retrieval-32M"


def load_model(model_id):
    from model2vec import StaticModel

    return StaticModel.from_pretrained(model_id)


def model_identifier(model_id, model):
    config = getattr(model, "config", None) or {}
    version = config.get("model2vec_version")
    if version:
        return f"{model_id}@{version}"
    return model_id
