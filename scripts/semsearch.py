# /// script
# requires-python = ">=3.10"
# dependencies = ["duckdb", "model2vec", "numpy"]
# ///
"""Semantic search over local files with DuckDB.

Usage:
    uv run semsearch.py "how do I cook pasta?" [glob] [limit]

Defaults: glob '**/*.md', limit 5.
"""
import sys

import duckdb
from model2vec import StaticModel

query = sys.argv[1]
pattern = sys.argv[2] if len(sys.argv) > 2 else "**/*.md"
limit = int(sys.argv[3]) if len(sys.argv) > 3 else 5

model = StaticModel.from_pretrained("minishlab/potion-base-8M")

con = duckdb.connect()
con.create_function(
    "embed",
    lambda t: [float(x) for x in model.encode([t])[0]],
    ["VARCHAR"],
    "FLOAT[256]",
)

rows = con.execute(
    """
    SELECT filename,
           round(array_cosine_distance(embed(content), embed(?)), 3) AS distance
    FROM read_text(?)
    ORDER BY distance
    LIMIT ?
    """,
    [query, pattern, limit],
).fetchall()

for filename, distance in rows:
    print(f"{distance:.3f}  {filename}")
