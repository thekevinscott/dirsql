### Small trees are walked on one thread

#### Summary

Directory walks of small trees (under about 128 directories) stay on the calling thread instead of spawning one worker per core. Results and ordering are unchanged; nothing breaks.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

_None._

#### Verification

Run `dirsql --help`; the CLI starts as before.
