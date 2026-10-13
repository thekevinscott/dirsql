### Frontend SQL execution

#### Summary

The `dirsql server --frontend` page can now run read-only SQL and show the result. It adds the `/frontend.js` route, served only with `--frontend`. Nothing breaks.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

_None._

#### Verification

Run `dirsql server --frontend --port 0`, open the printed URL, and press Run. The page lists the files under the current directory.
