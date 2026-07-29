"""The plugin's one configured cache.

Every cached function derives its own sub-cache from this singleton with
cachetta's ``cache / 'name'``, which keeps the defaults and appends a
subdirectory. Naming that subdirectory after the function is what keeps caches
apart: cachetta hashes the *arguments* of a call, not the identity of the
function, so two cached functions sharing a directory would collide as soon as
their argument shapes matched.

``path`` is resolved here rather than deferred -- ``__truediv__`` calls a
callable ``path`` and stores the result, so a lazy base would be frozen by the
first ``/`` anyway. The hooks are fresh subprocesses, so import time is process
start and ``DIRSQL_EMBEDDINGS_CACHE_DIR`` still takes effect.
"""

from cachetta import Cachetta

from .config import CACHE_DURATION, cache_dir

cache = Cachetta(path=cache_dir(), hashed=True, duration=CACHE_DURATION)
