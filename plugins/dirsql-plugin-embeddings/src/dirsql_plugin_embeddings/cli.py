import click

from .worker.cli import cli as worker


@click.group()
def main():
    """dirsql embeddings plugin."""


main.add_command(worker, name="worker")
