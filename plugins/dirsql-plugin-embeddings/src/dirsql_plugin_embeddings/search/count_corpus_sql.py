from .normalize_glob import normalize_glob
from .quote import quote


def count_corpus_sql(glob):
    return f"SELECT COUNT(*) AS n FROM {quote(normalize_glob(glob))}"
