import click

from .worker import worker


@click.group()
def main():
    """dirsql embeddings plugin."""


main.add_command(worker, name="worker")
