PATH_PREFIXES = ("./", "../", "/", "~/")


def quote(text):
    escaped = text.replace("'", "''")
    return f"'{escaped}'"


def normalize_glob(glob):
    # The core only rescues path-shaped missing tables (./, ../, /, ~/); a
    # bare relative glob like '**/*.md' would error with a "did you mean
    # './...'" hint. Here GLOB is unambiguously a corpus glob, so spare the
    # user the round trip.
    if glob.startswith(PATH_PREFIXES):
        return glob
    return f"./{glob}"


def embed_call(argument, model):
    if model is None:
        return f"embed({argument})"
    return f"embed({argument}, {quote(model)})"


def build_search_sql(glob, query, limit, model=None):
    outer = embed_call(quote(query), model)
    inner = embed_call("content", model)
    return (
        f"SELECT path, vec_distance_cosine(emb, {outer}) AS distance"
        f" FROM (SELECT path, {inner} AS emb FROM {quote(normalize_glob(glob))})"
        f" ORDER BY distance LIMIT {int(limit):d}"
    )
