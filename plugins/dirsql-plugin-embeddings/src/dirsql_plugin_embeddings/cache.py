"""The plugin's one configured cache.

Every cached function derives its own sub-cache from this singleton with
cachetta's ``cache / 'name'``, which keeps the defaults and appends a
subdirectory. Naming that subdirectory after the function is what keeps caches
apart: cachetta hashes the *arguments* of a call, not the identity of the
function, so two cached functions sharing a directory would collide as soon as
their argument shapes matched.
"""

from cachetta import Cachetta

from .config import CACHE_DIR, CACHE_DURATION, cache_reads_enabled

cache = Cachetta(
    path=CACHE_DIR,
    hashed=True,
    duration=CACHE_DURATION,
    read=cache_reads_enabled(),
)
