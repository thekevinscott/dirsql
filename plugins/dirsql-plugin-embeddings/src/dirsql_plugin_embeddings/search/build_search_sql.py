from .embed_call import embed_call
from .normalize_glob import normalize_glob
from .quote import quote


def build_search_sql(glob, query, limit, model=None):
    outer = embed_call(quote(query), model)
    inner = embed_call("content", model)
    # A matched file that is unreadable, not valid UTF-8, or deleted mid-scan
    # has NULL content, so its embedding is NULL too. Dropping those rows is
    # what keeps the ranking honest: a NULL distance sorts FIRST ascending in
    # SQLite, so unrankable files would otherwise take the top-k slots.
    return (
        f"SELECT path, vec_distance_cosine(emb, {outer}) AS distance"
        f" FROM (SELECT path, {inner} AS emb FROM {quote(normalize_glob(glob))})"
        f" WHERE emb IS NOT NULL"
        f" ORDER BY distance LIMIT {int(limit):d}"
    )
