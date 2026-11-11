### The watcher starts at build

#### Summary

Each handle with a named table starts its file watcher before the build walks the directory. Nothing breaks: the public API is unchanged.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

A change made while the build walks reaches the built index or the first poll rather than being missed.

#### Verification

_None._
