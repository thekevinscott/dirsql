# /// script
# requires-python = ">=3.10"
# dependencies = ["duckdb", "model2vec", "numpy"]
# ///
"""Semantic search over local files with DuckDB.

Usage:
    uv run semsearch.py "how do I cook pasta?"
    uv run semsearch.py "reviewing code" -g 'notes/**' -g 'docs/**/*.md' -n 3
"""
import argparse

import duckdb
from model2vec import StaticModel

parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
parser.add_argument("query", help="natural-language search query")
parser.add_argument(
    "-g",
    "--glob",
    action="append",
    help="glob of files to search; repeatable (default: **/*.md)",
)
parser.add_argument("-n", "--limit", type=int, default=5, help="rows to return (default: 5)")
args = parser.parse_args()

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
    [args.query, args.glob or ["**/*.md"], args.limit],
).fetchall()

for filename, distance in rows:
    print(f"{distance:.3f}  {filename}")
