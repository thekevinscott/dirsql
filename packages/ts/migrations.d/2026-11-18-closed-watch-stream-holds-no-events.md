### A watch stream holds no events once it is gone

#### Summary

A `watch()` stream that is closed, by leaving its `for await` loop, or dropped and garbage-collected, releases the events it holds, and while no stream is open no events are kept. No existing code has to change.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

A `watch()` called after every earlier stream was closed or collected sees only the changes made after that call. Changes made while no stream was open reach the index as before, and are reported to no stream. An instance on which `startWatcher()` or `pollEvents()` was called directly keeps its events for `pollEvents()` as before.

A stream's `return()` ends a `next()` that is waiting for an event, rather than waiting for that event to arrive. A `next()` that rejects closes the stream, as before.

#### Verification

In a process that stops reading a `watch()` stream while files keep changing, memory stays level once the loop is left or the stream is collected.
