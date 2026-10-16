"""Record everything a Python process reads, for the gate cache.

Imported by `scripts/gatecache_site/sitecustomize.py` into every Python process
a cached gate runs -- the checker itself and every Python process it starts --
when `GATE_CACHE_TRACE_DIR` is set. It changes nothing the process does; it
watches, and at exit writes one JSON file to the trace directory, which
`scripts/gate-cache.py` merges into the input set a cached verdict rests on.
Design: C-Q11 idea 2 (`requests/a-c-testing-without-a-full-boot-lane-a-takes-both.md`)
and design-decisions 974.

WHAT COUNTS AS AN INPUT
-----------------------
* A file opened for reading: its content, hashed when it is opened, so the hash
  is what the process read and not what the disk held later.
* A directory listed: the sorted (name, is-dir, is-file, is-symlink) of its
  entries, taken when it is listed. (`DirEntry.is_*` answers come from the same
  listing, so the digest covers them; `DirEntry.stat()` is recorded on its own.)
* A path asked about: the exists / isfile / isdir / islink / access answer, or
  (type, size, mtime) when the process read a whole `stat`.
* An environment variable read by name: its value, or its absence. A variable
  this run set, or that a parent set for this child, is not an input: its
  value came from the run.
* A child process. A Python one is traced the same way, its environment carrying
  the trace directory in. Read-only `git` against a repository the run did not
  create goes through `gatecache_tee.py`, which records the exact bytes it
  answered, and is re-run at lookup. `git` confined to directories the run
  created is the run's own computation, pinned by git's version and config.

The wall clock is deliberately not traced: `gate-cache.py` makes every entry
valid for the UTC day it was recorded, which bounds a date-dependent verdict
without patching `datetime` (a patched class changes reprs and pickling).

WHAT MAKES A RUN UNCACHEABLE
----------------------------
Anything whose input cannot be seen from here: any other executable, a shell
command line, a socket, native code (`ctypes`), the registry, a write outside
the directories the run itself created, and a traced process that did not
reach its exit handler. When in doubt the run is not stored. A cache that
misses costs a gate's normal run; a cache that hits wrongly passes a gate that
should have refused, and nothing downstream would notice.
"""

from __future__ import annotations

import atexit
import hashlib
import json
import os
import stat as stat_mod
import subprocess
import sys
import tempfile
import threading

_TRACE_DIR = os.environ.get("GATE_CACHE_TRACE_DIR", "")
_SPAWN_ID = os.environ.get("GATE_CACHE_SPAWN", "")

#: The variables a git child's behaviour depends on besides the repository
#: ones: where its global config lives, and which git runs.
GIT_ENV_NAMES = ("HOME", "USERPROFILE", "HOMEDRIVE", "HOMEPATH", "XDG_CONFIG_HOME",
                 "PATH", "LANG", "LC_ALL")
REPO_ENV_NAMES = ("GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE", "GIT_OBJECT_DIRECTORY",
                  "GIT_ALTERNATE_OBJECT_DIRECTORIES", "GIT_COMMON_DIR", "GIT_CONFIG",
                  "GIT_CONFIG_GLOBAL", "GIT_CONFIG_SYSTEM", "GIT_CONFIG_NOSYSTEM",
                  "GIT_TEMPLATE_DIR", "GIT_CEILING_DIRECTORIES", "GIT_NAMESPACE")
_PYTHON_NAMES = {"python", "python3", "pythonw", "py"}

#: git subcommands that only read. Any other one, run against a repository the
#: run did not create, changes that repository, and a replay would not.
GIT_READS = {
    "ls-files", "ls-tree", "cat-file", "rev-parse", "rev-list", "log", "show",
    "diff", "diff-tree", "diff-index", "diff-files", "merge-base", "for-each-ref",
    "show-ref", "symbolic-ref", "name-rev", "describe", "config", "check-attr",
    "check-ignore", "status", "blame", "grep", "count-objects", "var", "version",
    "worktree", "branch", "tag", "remote",
}
#: Words that turn one of those into a write.
GIT_WRITE_WORDS = {
    "config": {"--add", "--unset", "--unset-all", "--replace-all", "--rename-section",
               "--remove-section", "--edit", "-e"},
    "worktree": {"add", "remove", "prune", "move", "lock", "unlock", "repair"},
    "branch": {"-d", "-D", "-m", "-M", "-c", "-C", "--delete", "--move", "--copy",
               "--set-upstream-to", "-u", "--unset-upstream", "-f", "--force"},
    "tag": {"-d", "--delete", "-a", "-s", "-m", "-f", "--force", "-F"},
    "remote": {"add", "remove", "rm", "rename", "set-url", "set-head", "prune",
               "update", "set-branches"},
    "symbolic-ref": {"-d", "--delete"},
}

# The real functions, captured before anything is patched.
_REAL_OPEN = open
_REAL_SCANDIR = os.scandir
_REAL_STAT = os.stat
_REAL_LSTAT = os.lstat
_REAL_EXISTS = os.path.exists
_REAL_LEXISTS = os.path.lexists
_REAL_ISFILE = os.path.isfile
_REAL_ISDIR = os.path.isdir
_REAL_ISLINK = os.path.islink
_REAL_ACCESS = os.access


def norm(p) -> str | None:
    """The comparable form of a path: absolute, case-folded where the OS is."""
    if p is None or isinstance(p, int):
        return None
    try:
        return os.path.normcase(os.path.abspath(os.fsdecode(p)))
    except (TypeError, ValueError):
        return None


def sha256_file(path: str) -> str | None:
    """Content hash, or None when the path does not exist, or a marker."""
    h = hashlib.sha256()
    try:
        with _REAL_OPEN(path, "rb") as fh:
            while True:
                chunk = fh.read(1 << 20)
                if not chunk:
                    break
                h.update(chunk)
    except (FileNotFoundError, NotADirectoryError):
        return None
    except IsADirectoryError:
        return "dir"
    except PermissionError:
        # Windows answers a directory opened as a file this way too.
        return "dir" if _REAL_ISDIR(path) else "error:PermissionError"
    except OSError as exc:
        return f"error:{type(exc).__name__}"
    return h.hexdigest()


def file_stamp(path: str) -> list[int] | None:
    """(size, mtime_ns, file id) -- git's trick for not re-reading a file."""
    try:
        st = _REAL_STAT(path)
    except OSError:
        return None
    return [st.st_size, st.st_mtime_ns, st.st_ino]


def listing_digest(path: str) -> str | None:
    """Digest of a directory's entries, or None if it cannot be listed."""
    try:
        with _REAL_SCANDIR(path) as it:
            entries = []
            for e in it:
                try:
                    entries.append((e.name, e.is_dir(), e.is_file(), e.is_symlink()))
                except OSError:
                    entries.append((e.name, None, None, None))
    except OSError:
        return None
    entries.sort()
    return hashlib.sha256(
        json.dumps(entries).encode("utf-8", "surrogateescape")).hexdigest()


def stat_answer(path: str, kind: str):
    """The answer a stat-like query gives for `path` now. Both the recording
    and the lookup call this, so they cannot disagree about what a kind means."""
    try:
        if kind == "exists":
            return _REAL_EXISTS(path)
        if kind == "lexists":
            return _REAL_LEXISTS(path)
        if kind == "isfile":
            return _REAL_ISFILE(path)
        if kind == "isdir":
            return _REAL_ISDIR(path)
        if kind == "islink":
            return _REAL_ISLINK(path)
        if kind in ("full", "lfull"):
            st = (_REAL_STAT if kind == "full" else _REAL_LSTAT)(path)
            return [stat_mod.S_IFMT(st.st_mode), st.st_size, st.st_mtime_ns]
        if kind.startswith("access:"):
            return _REAL_ACCESS(path, int(kind.split(":", 1)[1]))
    except OSError:
        return None
    return None


def _is_devnull(raw) -> bool:
    try:
        s = os.fsdecode(raw).lower()
    except (TypeError, ValueError):
        return False
    return s in (os.devnull.lower(), "nul", "\\\\.\\nul", "/dev/null")


class _Tracer:
    def __init__(self, trace_dir: str, spawn_id: str) -> None:
        self.trace_dir = norm(trace_dir) or ""
        self.spawn_id = spawn_id
        self.tempdir = norm(tempfile.gettempdir()) or ""
        self.reads: dict[str, str | None] = {}
        self.lists: dict[str, str | None] = {}
        self.stats: dict[str, dict[str, object]] = {}
        self.env: dict[str, str | None] = {}
        self.env_bulk: list[str] = []
        self.internal_env: set[str] = set(json.loads(
            os.environ.get("GATE_CACHE_INTERNAL_ENV") or "[]"))
        self.roots: set[str] = set(json.loads(
            os.environ.get("GATE_CACHE_INTERNAL_ROOTS") or "[]"))
        self.made_dirs: set[str] = set()
        self.writes: set[str] = set()
        self.spawns: list[dict[str, object]] = []
        self.uncacheable: list[str] = []
        self.lock = threading.RLock()
        self.local = threading.local()
        self.counter = 0
        self.ended = False

    def busy(self) -> bool:
        return getattr(self.local, "busy", 0) > 0

    def enter(self) -> None:
        self.local.busy = getattr(self.local, "busy", 0) + 1

    def leave(self) -> None:
        self.local.busy -= 1

    def refuse(self, why: str) -> None:
        with self.lock:
            if why not in self.uncacheable:
                self.uncacheable.append(why)

    def is_internal(self, path: str) -> bool:
        if self.trace_dir and (path == self.trace_dir or path.startswith(self.trace_dir + os.sep)):
            return True
        if path in self.writes:
            return True
        for root in self.roots:
            if path == root or path.startswith(root.rstrip("\\/") + os.sep):
                return True
        return False

    def read(self, path: str) -> None:
        if "__pycache__" in path or self.is_internal(path):
            return
        with self.lock:
            if path in self.reads:
                return
        self.enter()
        try:
            stamp = file_stamp(path)
            digest = sha256_file(path)
        finally:
            self.leave()
        with self.lock:
            self.reads.setdefault(path, [digest, stamp])

    def listed(self, path: str) -> None:
        if self.is_internal(path):
            return
        with self.lock:
            if path in self.lists:
                return
        self.enter()
        try:
            digest = listing_digest(path)
        finally:
            self.leave()
        with self.lock:
            self.lists.setdefault(path, digest)

    def asked(self, path: str, kind: str) -> None:
        if self.is_internal(path):
            return
        with self.lock:
            if kind in self.stats.get(path, {}):
                return
        self.enter()
        try:
            answer = stat_answer(path, kind)
        finally:
            self.leave()
        with self.lock:
            self.stats.setdefault(path, {}).setdefault(kind, answer)

    def dump(self) -> None:
        if self.ended:
            return
        self.ended = True
        record = {
            "pid": os.getpid(),
            "spawn": self.spawn_id,
            "argv": list(sys.argv),
            "reads": self.reads,
            "lists": self.lists,
            "stats": self.stats,
            "env": self.env,
            "env_bulk": self.env_bulk,
            "writes": sorted(self.writes),
            "roots": sorted(self.roots),
            "made_dirs": sorted(self.made_dirs),
            "spawns": self.spawns,
            "uncacheable": self.uncacheable,
            "end": True,
        }
        self.enter()
        try:
            path = os.path.join(self.trace_dir, f"proc-{os.getpid()}-{id(self):x}.json")
            tmp = path + ".tmp"
            with _REAL_OPEN(tmp, "w", encoding="utf-8", newline="\n") as fh:
                json.dump(record, fh)
            os.replace(tmp, path)
        except OSError:
            pass  # no end record: the driver refuses to store the run
        finally:
            self.leave()


T: _Tracer | None = None

_WRITE_FLAGS = os.O_WRONLY | os.O_RDWR | os.O_CREAT | os.O_TRUNC | os.O_APPEND
#: Events that change a path, and which of their arguments are paths.
_WRITE_EVENTS = {
    "os.rmdir": (0,), "os.remove": (0,), "os.rename": (0, 1), "os.link": (0, 1),
    "os.symlink": (1,), "os.truncate": (0,), "os.chmod": (0,), "os.chown": (0,),
    "os.chflags": (0,), "os.utime": (0,), "os.setxattr": (0,),
    "os.removexattr": (0,), "shutil.copyfile": (1,), "shutil.copymode": (1,),
    "shutil.copystat": (1,), "shutil.copytree": (1,), "shutil.rmtree": (0,),
    "shutil.move": (0, 1), "shutil.chown": (0,),
}
#: Events whose inputs the tracer cannot see.
_UNSEEABLE = ("socket.", "ctypes.", "winreg.", "sqlite3.", "_winapi.CreateProcess",
              "os.system", "os.exec", "os.posix_spawn", "os.spawn", "os.startfile",
              "os.fork", "os.forkpty", "pty.spawn", "subprocess.Popen")


def _audit(event: str, args) -> None:
    t = T
    if t is None or t.busy():
        return
    try:
        if event == "open":
            if _is_devnull(args[0]):
                return
            path = norm(args[0])
            if path is None:
                return
            mode, flags = args[1], args[2]
            writing = (isinstance(mode, str) and any(c in mode for c in "wax+")) or (
                mode is None and isinstance(flags, int) and bool(flags & _WRITE_FLAGS))
            if writing:
                if not t.is_internal(path):
                    with t.lock:
                        t.writes.add(path)
                    t.refuse(f"writes {path}, outside the directories the run created")
                return
            t.read(path)
        elif event in ("os.listdir", "os.scandir"):
            path = norm(args[0] if args and args[0] is not None else ".")
            if path is not None:
                t.listed(path)
        elif event in ("tempfile.mkdtemp", "tempfile.mkstemp"):
            path = norm(args[0])
            if path:
                with t.lock:
                    t.roots.add(path)
                    # Uniquely named, so nothing else can depend on it; the
                    # os.mkdir that made it is not an output.
                    t.made_dirs.discard(path)
        elif event == "os.mkdir":
            path = norm(args[0])
            if path and not t.is_internal(path) and not _REAL_EXISTS(path):
                with t.lock:
                    t.roots.add(path)
                    # A directory made under a fixed name and still there
                    # after the run is an output a replay would not recreate;
                    # the driver checks at the end whether it is still there.
                    t.made_dirs.add(path)
        elif event in _WRITE_EVENTS:
            for i in _WRITE_EVENTS[event]:
                if i < len(args):
                    if _is_devnull(args[i]):
                        continue
                    path = norm(args[i])
                    if path and not t.is_internal(path):
                        t.refuse(f"{event} on {path}, outside the directories the run created")
        elif event.startswith(_UNSEEABLE):
            t.refuse(f"{event}: an input the tracer cannot see")
    except Exception as exc:  # noqa: BLE001 -- a tracer fault must not break the gate
        t.refuse(f"tracer fault on {event}: {type(exc).__name__}: {exc}")


def _query(real, kind):
    def traced(path, *a, **k):
        t = T
        if t is not None and not t.busy():
            p = norm(path)
            if p is not None:
                t.asked(p, kind)
        return real(path, *a, **k)
    traced.__wrapped__ = real
    return traced


def _traced_stat(path, *a, dir_fd=None, follow_symlinks=True, **k):
    t = T
    if t is not None and not t.busy() and dir_fd is None:
        p = norm(path)
        if p is not None:
            t.asked(p, "full" if follow_symlinks else "lfull")
    return _REAL_STAT(path, *a, dir_fd=dir_fd, follow_symlinks=follow_symlinks, **k)


def _traced_lstat(path, *a, dir_fd=None, **k):
    t = T
    if t is not None and not t.busy() and dir_fd is None:
        p = norm(path)
        if p is not None:
            t.asked(p, "lfull")
    return _REAL_LSTAT(path, *a, dir_fd=dir_fd, **k)


def _traced_access(path, mode, *a, **k):
    t = T
    if t is not None and not t.busy():
        p = norm(path)
        if p is not None:
            t.asked(p, f"access:{int(mode)}")
    return _REAL_ACCESS(path, mode, *a, **k)


class _Entry:
    """A `DirEntry` whose `stat()` is seen."""
    __slots__ = ("_e",)

    def __init__(self, e) -> None:
        self._e = e

    @property
    def name(self):
        return self._e.name

    @property
    def path(self):
        return self._e.path

    def inode(self):
        return self._e.inode()

    def is_dir(self, *, follow_symlinks=True):
        return self._e.is_dir(follow_symlinks=follow_symlinks)

    def is_file(self, *, follow_symlinks=True):
        return self._e.is_file(follow_symlinks=follow_symlinks)

    def is_symlink(self):
        return self._e.is_symlink()

    def is_junction(self):
        return self._e.is_junction()

    def stat(self, *, follow_symlinks=True):
        t = T
        if t is not None and not t.busy():
            p = norm(self._e.path)
            if p is not None:
                t.asked(p, "full" if follow_symlinks else "lfull")
        return self._e.stat(follow_symlinks=follow_symlinks)

    def __fspath__(self):
        return self._e.path

    def __repr__(self):
        return repr(self._e)


class _Scan:
    def __init__(self, it) -> None:
        self._it = it

    def __iter__(self):
        return self

    def __next__(self):
        return _Entry(next(self._it))

    def __enter__(self):
        return self

    def __exit__(self, *exc):
        self._it.close()
        return False

    def close(self):
        self._it.close()


def _traced_scandir(path="."):
    return _Scan(_REAL_SCANDIR(path))


def _patch_environ() -> None:
    cls = type(os.environ)
    real_getitem, real_setitem = cls.__getitem__, cls.__setitem__
    real_delitem, real_iter = cls.__delitem__, cls.__iter__

    def getitem(self, key):
        t = T
        watching = (t is not None and self is os.environ and not t.busy()
                    and not getattr(t.local, "child_env_copy", False))
        try:
            value = real_getitem(self, key)
        except KeyError:
            if watching:
                _note_env(t, key, None)
            raise
        if watching:
            _note_env(t, key, value)
        return value

    def setitem(self, key, value):
        t = T
        if t is not None and self is os.environ:
            with t.lock:
                t.internal_env.add(key)
        return real_setitem(self, key, value)

    def delitem(self, key):
        t = T
        if t is not None and self is os.environ:
            with t.lock:
                t.internal_env.add(key)
        return real_delitem(self, key)

    def iterate(self):
        t = T
        if t is not None and self is os.environ and not t.busy() \
                and not getattr(t.local, "child_env_copy", False):
            frame = sys._getframe(1)
            while frame is not None and (
                    frame.f_code.co_filename.startswith("<frozen ")
                    or frame.f_code.co_filename.endswith(
                        ("os.py", "_collections_abc.py", "gatecache_trace.py"))):
                frame = frame.f_back
            where = (f"{frame.f_code.co_filename}:{frame.f_lineno}"
                     if frame is not None else "?")
            with t.lock:
                if where not in t.env_bulk:
                    t.env_bulk.append(where)
        return real_iter(self)

    cls.__getitem__, cls.__setitem__ = getitem, setitem
    cls.__delitem__, cls.__iter__ = delitem, iterate


def _note_env(t: _Tracer, key, value) -> None:
    if key in t.internal_env or str(key).startswith("GATE_CACHE_"):
        return
    with t.lock:
        t.env.setdefault(key, value)


class child_env_copy:
    """Mark an environment copy as made for a child process, not read.

    `gitenv.clean_env` and `scrub_environ` copy or scan the environment to build
    a child's. The child's own reads are traced, and a git child's relevant
    variables are recorded where it starts, so the copy itself is not an input.
    Anything else that walks the environment records the whole of it."""

    def __enter__(self):
        t = T
        if t is not None:
            t.local.child_env_copy = True
        return self

    def __exit__(self, *exc):
        t = T
        if t is not None:
            t.local.child_env_copy = False
        return False


def is_python(exe: str) -> bool:
    base = os.path.basename(exe).lower()
    if base.endswith(".exe"):
        base = base[:-4]
    if base in _PYTHON_NAMES or base.startswith(("python3", "python2")):
        return True
    try:
        return os.path.normcase(os.path.abspath(exe)) == os.path.normcase(sys.executable)
    except (TypeError, ValueError):
        return False


def is_git(exe: str) -> bool:
    return os.path.basename(exe).lower() in ("git", "git.exe")


def git_subcommand(argv: list[str]) -> tuple[str | None, list[str]]:
    """(subcommand, the words after it), skipping git's own options."""
    takes_value = {"-C", "-c", "--git-dir", "--work-tree", "--namespace",
                   "--exec-path", "--super-prefix", "--config-env"}
    i = 1
    while i < len(argv):
        a = argv[i]
        if a in takes_value:
            i += 2
        elif a.startswith("-"):
            i += 1
        else:
            return a, argv[i + 1:]
    return None, []


def git_path_words(argv: list[str], cwd: str) -> list[str]:
    """Every word of a git command line that names a path, resolved."""
    out = []
    i = 1
    while i < len(argv):
        a = argv[i]
        if a in ("-C", "--git-dir", "--work-tree", "--exec-path") and i + 1 < len(argv):
            val = argv[i + 1]
            i += 2
        elif a == "-c" and i + 1 < len(argv):
            val = argv[i + 1].split("=", 1)[1] if "=" in argv[i + 1] else None
            i += 2
        elif a.startswith("--") and "=" in a:
            val = a.split("=", 1)[1]
            i += 1
        else:
            val = a
            i += 1
        if not val or val.startswith("-"):
            continue
        if "/" in val or "\\" in val or os.path.isabs(val) or \
                _REAL_EXISTS(os.path.join(cwd, val)):
            out.append(norm(os.path.join(cwd, val)))
    return [p for p in out if p]


def _patch_popen() -> None:
    real_init = subprocess.Popen.__init__
    here = os.path.dirname(os.path.abspath(__file__))
    site_dir = os.path.join(here, "gatecache_site")
    tee = os.path.join(here, "gatecache_tee.py")

    def init(self, args, *a, **k):
        t = T
        if t is None or t.busy():
            return real_init(self, args, *a, **k)
        t.enter()
        try:
            args, k = _classify(t, args, a, k, site_dir, tee)
            return real_init(self, args, **k)
        finally:
            t.leave()

    subprocess.Popen.__init__ = init


_POPEN_POSITIONAL = ("bufsize", "executable", "stdin", "stdout", "stderr",
                     "preexec_fn", "close_fds", "shell", "cwd", "env")


def _classify(t: _Tracer, args, a, k, site_dir: str, tee: str):
    """Decide what kind of child this is, adjust its launch, and record it.
    Runs with the tracer busy, so its own environment reads are not inputs."""
    for name, value in zip(_POPEN_POSITIONAL, a):
        k.setdefault(name, value)
    shell = bool(k.get("shell"))
    cwd = os.path.abspath(os.fsdecode(k["cwd"])) if k.get("cwd") else os.getcwd()
    env = k.get("env")
    if isinstance(args, (str, bytes)):
        argv = None if shell else [os.fsdecode(args)]
    else:
        argv = [os.fsdecode(x) for x in args]
    exe = os.fsdecode(k["executable"]) if k.get("executable") else (argv[0] if argv else "")
    with t.lock:
        t.counter += 1
        sid = f"{t.spawn_id or 'root'}.{t.counter}"
    base_env = dict(os.environ) if env is None else {
        os.fsdecode(n): os.fsdecode(v) for n, v in env.items()}
    overlay = sorted(n for n in set(base_env) | set(os.environ)
                     if base_env.get(n) != os.environ.get(n))

    if shell or argv is None:
        t.refuse(f"a shell command line: {args!r}")
    elif is_python(exe):
        child = dict(base_env)
        child["GATE_CACHE_TRACE_DIR"] = t.trace_dir
        child["GATE_CACHE_SPAWN"] = sid
        child["GATE_CACHE_INTERNAL_ENV"] = json.dumps(sorted(
            set(overlay) | t.internal_env | {"PYTHONPATH"}))
        child["GATE_CACHE_INTERNAL_ROOTS"] = json.dumps(sorted(t.roots))
        child["PYTHONPATH"] = site_dir + (
            os.pathsep + base_env["PYTHONPATH"] if base_env.get("PYTHONPATH") else "")
        k["env"] = child
        with t.lock:
            t.spawns.append({"id": sid, "kind": "python", "argv": argv, "cwd": cwd})
    elif is_git(exe):
        paths = [norm(cwd)] + git_path_words(argv, cwd)
        for n in REPO_ENV_NAMES:
            v = base_env.get(n)
            if v and n not in ("GIT_CONFIG_NOSYSTEM", "GIT_NAMESPACE"):
                paths.append(norm(v))
        git_env = {n: base_env.get(n) for n in REPO_ENV_NAMES + GIT_ENV_NAMES}
        # The git-relevant variables this child inherited unchanged come from
        # the environment the gate was started in, so they are inputs, to be
        # compared at lookup. The ones this process set are its computation,
        # and the replay uses the recorded values.
        for n in REPO_ENV_NAMES + GIT_ENV_NAMES:
            if n not in overlay:
                _note_env(t, n, os.environ.get(n))
        if all(p and t.is_internal(p) for p in paths):
            with t.lock:
                t.spawns.append({"id": sid, "kind": "git-internal", "argv": argv,
                                 "env": git_env})
        else:
            sub, rest = git_subcommand(argv)
            reads_stdin = any(w == "--stdin" or w.startswith("--batch") for w in rest)
            piped = k.get("stdin") is not None
            if sub not in GIT_READS or set(rest) & GIT_WRITE_WORDS.get(sub, set()):
                t.refuse(f"git {sub} against a repository the run did not create")
            elif reads_stdin and not piped:
                t.refuse(f"git {sub} reads an inherited stdin, which cannot be recorded")
            else:
                record = os.path.join(t.trace_dir, f"git-{sid}.json")
                k.pop("executable", None)
                args = [sys.executable, tee, record, "piped" if piped else "none",
                        "--"] + argv
                # The tee itself is not traced, or it would trace, and tee,
                # its own git call.
                tee_env = dict(base_env)
                tee_env["GATE_CACHE_TRACE_DIR"] = ""
                k["env"] = tee_env
                with t.lock:
                    t.spawns.append({"id": sid, "kind": "git-read", "argv": argv,
                                     "cwd": cwd, "record": record, "env": git_env})
    else:
        t.refuse(f"runs {os.path.basename(exe) or exe!r}, whose reads cannot be seen")
    return args, k


def install() -> None:
    """Start tracing this process. Idempotent; a no-op without a trace dir."""
    global T
    if T is not None or not _TRACE_DIR:
        return
    T = _Tracer(_TRACE_DIR, _SPAWN_ID)
    os.stat = _traced_stat
    os.lstat = _traced_lstat
    os.access = _traced_access
    os.scandir = _traced_scandir
    os.path.exists = _query(_REAL_EXISTS, "exists")
    os.path.lexists = _query(_REAL_LEXISTS, "lexists")
    os.path.isfile = _query(_REAL_ISFILE, "isfile")
    os.path.isdir = _query(_REAL_ISDIR, "isdir")
    os.path.islink = _query(_REAL_ISLINK, "islink")
    _patch_environ()
    _patch_popen()
    sys.addaudithook(_audit)
    atexit.register(T.dump)
