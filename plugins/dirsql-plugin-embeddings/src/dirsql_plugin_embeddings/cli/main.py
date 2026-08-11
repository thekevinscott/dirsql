import click

from .search import search
from .worker import worker


class DefaultCommandGroup(click.Group):
    """Route non-subcommand invocations to the search command.

    Bare positionals are the plugin's primary interface
    (`dirsql-plugin-embeddings '<glob>' '<query>'`), so anything that is not
    a known subcommand or the group's own --help is re-parsed as arguments
    to `search`.
    """

    def parse_args(self, ctx, args):
        if not args or (args[0] != "--help" and args[0] not in self.commands):
            args = [search.name, *args]
        return super().parse_args(ctx, args)


@click.group(cls=DefaultCommandGroup)
def main():
    """dirsql embeddings plugin.

    Bare arguments run the search command:
    dirsql-plugin-embeddings '<glob>' '<query>' [-k N] [--model ID]
    """


main.add_command(worker, name="worker")
main.add_command(search, name="search")
