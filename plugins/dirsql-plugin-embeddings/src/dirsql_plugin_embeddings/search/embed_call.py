from .quote import quote


def embed_call(argument, model):
    if model is None:
        return f"embed({argument})"
    return f"embed({argument}, {quote(model)})"
