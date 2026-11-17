### Windows watcher no longer hangs a directory delete after drop

#### Summary

On Windows, removing a directory just after the `DirSQL` watching it was dropped could block forever. The `notify` dependency moves from 7 to 8 to fix it. Nothing breaks and no code changes are required.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

_None._

#### Verification

On Windows, build a `DirSQL` over a temporary directory, drop it, and remove the directory at once, in a loop; every iteration returns.
