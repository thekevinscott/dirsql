import click

from .search import search
from .worker import worker


class DefaultCommandGroup(click.Group):
    """Route non-`worker` invocations to the hidden search command.

    Bare positionals are the plugin's only search interface
    (`dirsql-plugin-embeddings '<glob>' '<query>'`): a first token counts as
    a subcommand only when it names a *visible* command, so a literal
    'search' first token is a corpus glob, not a spelling of the command.
    """

    def parse_args(self, ctx, args):
        visible = [
            name
            for name, command in self.commands.items()
            if not command.hidden
        ]
        if not args or (args[0] != "--help" and args[0] not in visible):
            args = [search.name, *args]
        return super().parse_args(ctx, args)


@click.group(cls=DefaultCommandGroup)
def main():
    """dirsql embeddings plugin.

    Bare arguments run the semantic search:
    dirsql-plugin-embeddings '<glob>' '<query>' [-k N] [--model ID]
    """


main.add_command(worker, name="worker")
main.add_command(search, name="search")
