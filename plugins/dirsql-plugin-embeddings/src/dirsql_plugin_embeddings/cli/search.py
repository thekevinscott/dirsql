import click

from ..search.run import NothingToRank, run_search


@click.command(hidden=True)
@click.argument("glob")
@click.argument("query")
@click.option(
    "-k",
    "--limit",
    "limit",
    type=int,
    default=10,
    show_default=True,
    help="Maximum results; exactly the SQL LIMIT.",
)
@click.option(
    "--model",
    default=None,
    help="model2vec model id, templated as embed()'s second argument.",
)
def search(glob, query, limit, model):
    """Rank files matching GLOB by semantic similarity to QUERY."""
    try:
        lines = run_search(glob, query, limit, model)
    except NothingToRank as error:
        click.echo(f"dirsql-plugin-embeddings: {error}", err=True)
        raise SystemExit(1) from None
    for line in lines:
        click.echo(line)
