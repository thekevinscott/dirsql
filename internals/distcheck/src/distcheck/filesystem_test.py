import os
import stat

from distcheck.filesystem import FileSystem


def test_exists_reflects_presence(tmp_path):
    fs = FileSystem()
    assert fs.exists(str(tmp_path)) is True
    assert fs.exists(str(tmp_path / "missing")) is False


def test_makedirs_creates_nested_and_is_idempotent(tmp_path):
    fs = FileSystem()
    target = str(tmp_path / "a" / "b")
    fs.makedirs(target)
    fs.makedirs(target)  # exist_ok -- second call must not raise
    assert os.path.isdir(target)


def test_copy_duplicates_file_contents(tmp_path):
    fs = FileSystem()
    src = tmp_path / "src.bin"
    src.write_text("payload")
    dst = str(tmp_path / "dst.bin")
    fs.copy(str(src), dst)
    assert open(dst, encoding="utf-8").read() == "payload"


def test_chmod_sets_mode(tmp_path):
    fs = FileSystem()
    target = tmp_path / "bin"
    target.write_text("")
    fs.chmod(str(target), 0o755)
    assert stat.S_IMODE(os.stat(target).st_mode) == 0o755


def test_listdir_lists_entries(tmp_path):
    fs = FileSystem()
    (tmp_path / "one").write_text("")
    (tmp_path / "two").write_text("")
    assert sorted(fs.listdir(str(tmp_path))) == ["one", "two"]


def test_mkdtemp_makes_a_real_prefixed_dir():
    fs = FileSystem()
    created = fs.mkdtemp("dirsql-distcheck-test-")
    try:
        assert os.path.isdir(created)
        assert os.path.basename(created).startswith("dirsql-distcheck-test-")
    finally:
        fs.rmtree(created)


def test_rmtree_removes_tree_and_ignores_missing(tmp_path):
    fs = FileSystem()
    tree = tmp_path / "tree"
    (tree / "nested").mkdir(parents=True)
    fs.rmtree(str(tree))
    assert not os.path.exists(tree)
    fs.rmtree(str(tree))  # ignore_errors -- second call must not raise


def test_read_text_round_trips_write_text(tmp_path):
    fs = FileSystem()
    target = str(tmp_path / "note.txt")
    fs.write_text(target, "hello")
    assert fs.read_text(target) == "hello"
