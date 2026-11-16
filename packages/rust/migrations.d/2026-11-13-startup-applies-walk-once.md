### Startup applies the changes from the walk once

#### Summary

`DirSQL::new` and the builder no longer wait for the watcher's queue to come back empty before returning. The changes that arrived during the initial walk are applied once; anything later is delivered by the watcher as before. Nothing breaks.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

A file changed while startup applies the walk's changes reaches the first poll or watch rather than the built index.

#### Verification

_None._
