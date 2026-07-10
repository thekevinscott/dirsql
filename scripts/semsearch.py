# /// script
# requires-python = ">=3.10"
# dependencies = ["duckdb", "model2vec", "numpy"]
# ///
"""Semantic search over local files with DuckDB.

Files are split into paragraphs, each paragraph is embedded separately, and a
file is ranked by its best-matching paragraph (whole-document embeddings
average long files into mush).

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
needle = [float(x) for x in model.encode([args.query])[0]]

rows = con.execute(
    """
    WITH chunks AS (
        SELECT filename,
               unnest(string_split_regex(content, '\n[ \t]*\n')) AS chunk
        FROM read_text(?)
    ),
    scored AS (
        SELECT filename, chunk,
               array_cosine_distance(embed(chunk), ?::FLOAT[256]) AS distance
        FROM chunks
        WHERE length(trim(chunk)) > 0
    )
    SELECT filename,
           round(min(distance), 3) AS distance,
           arg_min(chunk, distance) AS best_chunk
    FROM scored
    GROUP BY filename
    ORDER BY distance
    LIMIT ?
    """,
    [args.glob or ["**/*.md"], needle, args.limit],
).fetchall()

for filename, distance, chunk in rows:
    snippet = " ".join(chunk.split())[:100]
    print(f"{distance:.3f}  {filename}")
    print(f"       {snippet}")
