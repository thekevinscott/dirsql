# Source Size

Two rules for judging a change that claims to shrink the codebase. Both come out of epic #1072, which set out to cut source from ~10,400 lines to 8,500 and instead ended at ~10,160 with the candidate pool exhausted.

## Score consolidation at zero by default

If a candidate says "move this into the core" or "adopt a crate for this", the prior on its line savings is **zero** until measured otherwise. Only deletion of whole logic reliably shrinks the tree.

Measured repo-wide before and after each merge:

| | estimated | actual |
| --- | ---: | ---: |
| three deletions (#1073, #1074, #1075) | -620 | **-698** |
| three consolidations (#1078, #1080, #1081) | -485 | **+289** |

Every deletion beat its estimate. Every consolidation missed, #1078 by 421 lines. A consolidation is a relocation: logic leaves the bindings and lands in `packages/rust/src` (+317 over the epic), and the destination needs a module header, a test module, and usually an adapter. #1081 is the sharpest case -- indicatif refuses to draw to a non-terminal, so preserving an existing test contract needed a bespoke `TermLike`/`SinkTerm` shim, and `progress.rs` ended up larger (239 -> 305) than the hand-rolled code it replaced.

## Flat is itself a ratchet

Holding the line takes active resistance, because growth is the default. +82 of the core's growth over the epic was two unrelated watcher bug fixes (#1096, #1109) landing while the epic was in flight -- ordinary correct work, no part of it wrong, and it offset a third of what the deletions won.

## Scope, if the question comes up again

The epic's constraints were: the public API does not change, and no existing test is rewritten or weakened. Those ruled out the largest remaining candidates -- adopting `comfy-table` in `cli/table.rs` changes rendered output, which changes test assertions. A candidate that only shrinks the number by cutting gate coverage is not a candidate; `wheel-extension-load` (120 lines) survives on exactly that ground, since it installs the release-shape manylinux wheel that `internals/distcheck` never sees.

The one-time measurement tooling built for the epic was deliberately not kept.
