### A build's JS heap no longer grows with its file count

#### Summary

Every `onFile` result stayed reachable until the build returned, so a build over enough files exhausted the Node heap. Each call's allocations are now released when it returns. Nothing breaks and no code changes are required.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

A build's JS heap stays flat as its file count grows, with or without `persist`.

#### Verification

Build a `DirSQL` with an `onFile` table over 40,000 files and sample `process.memoryUsage().heapUsed` inside `onFile`: it stays within a few MB of its starting value instead of growing by about 1.5 KB per file.
