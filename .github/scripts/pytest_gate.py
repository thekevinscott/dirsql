import subprocess
import sys
from pathlib import Path

NO_TESTS_COLLECTED = 5


def find_test_files(paths):
    matches = []
    for path in paths:
        matches.extend(Path(path).rglob("*_test.py"))
    return matches


def interpret(returncode):
    if returncode == NO_TESTS_COLLECTED:
        return 0
    return returncode


def main(argv, runner=subprocess.run, finder=find_test_files):
    paths = [arg for arg in argv if not arg.startswith("-")]
    if not finder(paths):
        print(f"No *_test.py under {paths or ['.']} — nothing to test.")
        return 0
    result = runner([sys.executable, "-m", "pytest", *argv])
    return interpret(result.returncode)


if __name__ == "__main__":  # pragma: no cover
    raise SystemExit(main(sys.argv[1:]))
