### A missing index root fails the build

#### Summary

Building a `DirSQL` (Rust, Python, TypeScript) whose index root does not exist now fails with `root directory <dir> does not exist`. Builds that used to succeed over a missing root now fail. The CLI always roots at the current directory, so it is unaffected.

#### Required changes

| Before | After |
| --- | --- |
| `DirSQL("/path/not/yet/created")` built an empty instance | Create the directory before building |

#### Deprecations removed

_None._

#### Behavior changes without code changes

The build raises instead of succeeding. Before, it also started no file watcher, so files created under the root later went unseen.

#### Verification

In Python, `await DirSQL("/no/such/dir").ready()` inside a coroutine raises `RuntimeError: root directory /no/such/dir does not exist`.
