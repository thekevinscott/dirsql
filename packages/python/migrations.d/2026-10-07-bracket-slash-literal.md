### A slash inside brackets is literal

#### Summary

A slash inside brackets ends the component, so the brackets are literal, as in bash.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

- A `/` inside `[...]` is literal and ends the component, as in bash: `./[b/c]/*` matches nothing instead of `b/c/*`.

#### Verification

```bash
dirsql query "SELECT path FROM './[d/x]ocs/api.md'"
```
