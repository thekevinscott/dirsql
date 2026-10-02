import sqlite3
from array import array
from hashlib import sha256
from json import dumps

SCHEMA = (
    "CREATE TABLE IF NOT EXISTS vectors"
    " (digest TEXT PRIMARY KEY, vector BLOB NOT NULL)"
)
LOOKUP = (
    "SELECT digest, vector FROM vectors"
    " WHERE digest IN (SELECT value FROM json_each(?))"
)
INSERT = "INSERT OR REPLACE INTO vectors (digest, vector) VALUES (?, ?)"


def pack(vector):
    return array("d", vector).tobytes()


def unpack(blob):
    doubles = array("d")
    doubles.frombytes(blob)
    return doubles.tolist()


class VectorStore:
    """One SQLite database per model under ``directory``, keyed by text digest."""

    def __init__(self, directory):
        self._directory = directory
        self._connections = {}

    def _connection(self, identifier):
        if identifier not in self._connections:
            self._directory.mkdir(parents=True, exist_ok=True)
            name = sha256(identifier.encode("utf-8")).hexdigest() + ".db"
            connection = sqlite3.connect(self._directory / name)
            connection.execute(SCHEMA)
            self._connections[identifier] = connection
        return self._connections[identifier]

    def lookup(self, identifier, digests):
        """The stored vectors among ``digests``, keyed by digest."""
        rows = self._connection(identifier).execute(LOOKUP, (dumps(list(digests)),))
        return {digest: unpack(blob) for digest, blob in rows}

    def insert(self, identifier, vectors):
        """Store ``vectors`` (digest to vector) in one transaction."""
        connection = self._connection(identifier)
        rows = [(digest, pack(vector)) for digest, vector in vectors.items()]
        with connection:
            connection.executemany(INSERT, rows)
