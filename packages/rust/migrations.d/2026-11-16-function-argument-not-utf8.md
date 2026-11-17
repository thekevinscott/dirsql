### A function argument that is not valid UTF-8 fails the call

#### Summary

A `[[dirsql.function]]` call whose argument holds text that is not valid UTF-8 now fails with `argument <n> of "<function>" holds text that is not valid UTF-8` (`<n>` counts from 1) instead of panicking with `unwinding panic`. The worker is never sent the call. Calls with valid arguments are unchanged; nothing breaks.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

_None._

#### Verification

With a function `up` declared, `dirsql query "SELECT up(CAST(x'ff41' AS TEXT))"` exits non-zero and prints `argument 1 of "up" holds text that is not valid UTF-8` on stderr, with no panic message.
