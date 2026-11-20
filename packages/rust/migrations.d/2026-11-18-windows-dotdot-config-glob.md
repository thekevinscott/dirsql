### Rust core: `../` config globs anchor correctly under a verbatim config path

**Summary**

On Windows, a config glob such as `../data/*.json` loaded from a verbatim (`\\?\`) config path now anchors at the config directory's parent, as it already did on other platforms and for non-verbatim paths. A bug fix; nothing breaks.

**Required changes**

_None._

**Deprecations removed**

_None._

**Behavior changes without code changes**

On Windows, a `../` config glob under a verbatim config path that used to match nothing now matches the files it names.

**Verification**

```bash
cargo test -p dirsql --features cli --test cli_config_anchor
# test result: ok.
```
