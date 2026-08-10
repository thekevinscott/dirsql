import argparse
import sys

from .progress import configure
from .worker import Worker


def main(argv=None):
    parser = argparse.ArgumentParser(prog="dirsql-plugin-embeddings")
    subcommands = parser.add_subparsers(dest="command", required=True)
    subcommands.add_parser(
        "worker",
        help="serve embed() requests as newline-delimited JSON on stdin/stdout",
    )
    parser.parse_args(argv)
    configure()
    Worker().serve(sys.stdin, sys.stdout)
    return 0
