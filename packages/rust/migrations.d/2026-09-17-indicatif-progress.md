## Summary

Progress drawing moved from a hand-rolled `\r`-rewritten counter to
`indicatif`. No public API changed: `PROGRESS_ENV`, `Mode`, `Progress::new`,
`update`, `finish` and `restart` all keep their signatures and meanings. What
changed is the byte stream a forced-on run writes.

## Required changes

_None._ Nothing to edit in any consumer.

## Deprecations removed

_None._

## Behavior changes without code changes

- **`DIRSQL_PROGRESS=always` writes a different byte stream.** Each frame is
  now padded out to the full line width (the terminal's, or 80 columns when
  there is no terminal) and each redraw is preceded by a blanked line, where
  the old reporter padded only over the tail of the line it replaced. Anything
  parsing that stream with an exact-match assertion needs to match on content
  instead; `contains`-style assertions are unaffected. The stream still carries
  no ANSI escape sequences, and the summary line is still newline-terminated.
- **A fast phase emits more frames.** `indicatif`'s rate limiter is a leaky
  bucket with a burst allowance rather than a hard 100 ms gate, so a phase that
  ends within the first second draws up to 20 frames where the old throttle
  drew one or two. Long phases settle to the same 10 frames a second.
- **Unchanged, and checked byte-for-byte:** the default (`auto`) writes nothing
  when stderr is not a terminal, `never` writes nothing ever, stdout is
  untouched in every mode, the `auto` warmup is still 500 ms, and percentages
  still floor rather than round.
- The published crate gains `indicatif` and `console` as non-optional
  dependencies. `cargo add dirsql` pulls four crates it did not before.

## Verification

```console
$ DIRSQL_PROGRESS=always dirsql query "SELECT count(*) FROM items" 2>run.log
$ grep -c $'\x1b' run.log      # 0 -- no escape sequences in a redirected run
$ tail -c 40 run.log           # ends with a newline-terminated summary
dirsql: indexed 200 files in 0.1s
$ dirsql query "SELECT count(*) FROM items" 2>silent.log; wc -c < silent.log
0
```
