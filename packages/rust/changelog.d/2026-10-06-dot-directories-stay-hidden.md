**Fixed** A path-table glob enters a dot-named directory only where a
component spells it at that depth, as bash does. `'./**/.*'` used to list
dotfiles inside dot-named directories such as `.cache/`; it now lists only
dot-named files whose parent directories are not themselves dot-named.
