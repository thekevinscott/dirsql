### A watch stream can be closed, and holds no events once it is gone

#### Summary

A `watch()` stream gains `aclose()` and `async with`. A stream that is closed, or dropped and collected, releases the events it holds, and while no stream is open no events are kept. No existing code has to change.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

A `watch()` called after every earlier stream was closed or collected sees only the changes made after that call. Changes made while no stream was open reach the index as before, and are reported to no stream.

#### Verification

In a process that stops reading a `watch()` stream while files keep changing, memory stays level once the stream is closed or collected.
