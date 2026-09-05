def model_identifier(model_id, model):
    config = getattr(model, "config", None) or {}
    version = config.get("model2vec_version")
    if version:
        return f"{model_id}@{version}"
    return model_id
