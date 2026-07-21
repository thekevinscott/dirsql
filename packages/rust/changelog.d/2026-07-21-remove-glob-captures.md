**Removed**

Glob `{name}` placeholders no longer populate DDL columns (the capture-extraction path is gone). `{name}` remains valid match syntax and behaves like `*`. A config declaring a `{name}` placeholder whose name is also a declared DDL column is now a **load-time error** naming the placeholder, the colliding column, and the fix (emit the value from the on-file hook by splitting `{path}` yourself). A `{name}` placeholder with no matching column keeps working silently. The public `matcher` API drops `MatchResult.captures`, `parse_captures`, and `captures_for`.
