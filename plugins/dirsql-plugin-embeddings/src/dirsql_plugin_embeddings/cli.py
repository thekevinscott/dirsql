import sys

import click

from .progress import configure
from .worker import Worker


@click.group()
def main():
    """dirsql embeddings plugin."""


@main.command()
def worker():
    """Serve embed() requests as newline-delimited JSON on stdin/stdout."""
    configure()
    Worker().serve(sys.stdin, sys.stdout)
