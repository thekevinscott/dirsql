**Changed**

- **An index root that does not exist now fails the build**, with `root directory <dir> does not exist`. Before, the build succeeded with no file watcher, so the instance stayed empty and later files under the root went unseen. Every SDK raises the error.
