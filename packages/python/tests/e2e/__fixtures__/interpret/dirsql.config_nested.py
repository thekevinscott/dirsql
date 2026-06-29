"""Fixture: a native Python config that itself sets ``config=``.

`dirsql interpret` must reject this -- a config file loaded by interpret
cannot delegate to another config file (nested config loading). The
referenced TOML is valid, so the rejection comes from the loader, not from
a TOML read error.
"""

import os

from dirsql import DirSQL

app = DirSQL(config=os.path.join(os.path.dirname(__file__), "nested.dirsql.toml"))
