import click

from ..search.run import run_search


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
    for line in run_search(glob, query, limit, model):
        click.echo(line)
