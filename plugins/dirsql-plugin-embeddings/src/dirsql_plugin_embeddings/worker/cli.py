import sys

import click

from ..progress import configure
from .worker import Worker


@click.command()
def cli():
    """Serve embed() requests as newline-delimited JSON on stdin/stdout."""
    configure()
    Worker().serve(sys.stdin, sys.stdout)
