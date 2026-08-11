import sys

import click

from ..embedding.progress import configure
from ..embedding.worker import Worker


@click.command()
def worker():
    """Serve embed() requests as newline-delimited JSON on stdin/stdout."""
    configure()
    Worker().serve(sys.stdin, sys.stdout)
