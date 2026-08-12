import os
import sys
import threading

from tqdm import tqdm


def stderr_is_tty():
    return sys.stderr.isatty()


def configure():
    # Hugging Face hub download bars honor this variable; setdefault keeps an
    # explicit user choice in force.
    if not stderr_is_tty():
        os.environ.setdefault("HF_HUB_DISABLE_PROGRESS_BARS", "1")
    # tqdm's first bar (model2vec's encode loop constructs one even when
    # disabled) lazily allocates a multiprocessing RLock -- a named semaphore
    # registered with the resource tracker. The dirsql core tears this worker
    # down with a kill, not a graceful exit, so that semaphore is never
    # unregistered and the tracker prints a "leaked semaphore" warning after
    # every run. A single worker process needs no cross-process bar
    # coordination: a process-local lock means no semaphore exists at all.
    tqdm.set_lock(threading.RLock())
