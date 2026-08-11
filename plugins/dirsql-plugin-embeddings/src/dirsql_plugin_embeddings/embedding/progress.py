import os
import sys


def stderr_is_tty():
    return sys.stderr.isatty()


def configure():
    # Hugging Face hub download bars honor this variable; setdefault keeps an
    # explicit user choice in force.
    if not stderr_is_tty():
        os.environ.setdefault("HF_HUB_DISABLE_PROGRESS_BARS", "1")
