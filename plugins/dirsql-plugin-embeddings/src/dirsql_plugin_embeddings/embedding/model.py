DEFAULT_MODEL_ID = "minishlab/potion-retrieval-32M"


def load_model(model_id):
    from model2vec import StaticModel

    return StaticModel.from_pretrained(model_id)
