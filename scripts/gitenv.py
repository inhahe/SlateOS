#!/usr/bin/env python3
"""Choosing which git repository a subprocess talks to.

Why this module exists
----------------------

`git -C <dir>` and `subprocess.run(..., cwd=<dir>)` both look like they name a
repository. Neither does. An explicit ``GIT_DIR`` in the environment outranks
both, and git *exports* ``GIT_DIR`` -- along with ``GIT_INDEX_FILE``,
``GIT_OBJECT_DIRECTORY`` and friends -- into the environment of:

* every **hook** (`pre-push`, `pre-commit`, `post-checkout`, ...),
* ``git bisect run <cmd>``,
* ``git rebase --exec <cmd>``,
* ``git filter-branch`` / filter-repo callbacks,
* ``git submodule foreach``.

So a program that builds a throwaway repository in a temp directory and drives
it with `-C` is correct when a human runs it and wrong the first time anything
in that list runs it -- and it is wrong in the worst available way, because it
writes to the repository it was supposed to be leaving alone while reporting
success about the fixture it thought it was using.

That is not a caution, it is a post-mortem. On 2026-08-29 `pre-push` gained a
gate that runs ``check-requests-not-deleted.py --selftest`` before trusting the
checker. The self-test builds a fixture with ``git init`` / ``git add -A`` /
``git commit`` in a `tempfile.TemporaryDirectory`, each with ``cwd=<tmp>``. On
its first real invocation every one of those commands operated on the
repository being pushed:

* ``git init`` re-initialised it and set ``core.bare=true`` on the shared
  config, which made the `os` integration worktree unusable (`git status`
  answers "this operation must be run in a work tree");
* ``git add -A`` replaced the index with the fixture's three files;
* ``git commit`` wrote ``7f6a6b446`` and ``71f164f7e`` onto `lane-a`, whose
  tree is a single `requests/` directory -- the entire repository deleted.

Both commits were then published to `origin/lane-a` and `origin/main`, because
the gate passed: it had correctly verified a fixture, and the fixture was the
repository. Repaired in ``f0534726e``; regression-tested in
`scripts/test-check-requests-not-deleted.py`.

It happened again on 2026-09-26, and the second time is the argument for a net
rather than a warning. ``check-release-staleness.py`` gained a ``--selftest``
that builds a scratch repository to count real commits, written without this
module. Its first run inside ``pre-push`` wrote seven fixture commits onto
`lane-a` -- ``{}`` over ``bench/boot-history.jsonl`` and
``bench/last-release-boot.json``, ``x = 1`` over ``bench/baselines.toml``. The
push had already resolved its ref, so nothing was published; the boot test
started beside it found ``scripts/x.py`` tracked and missing, and the branch was
rewound. Repaired in ``bc1bbad54`` (``clean_env()`` everywhere, and a
second-line check that ``git rev-parse --absolute-git-dir`` is the scratch
directory's own before the first write -- the git dir, not ``--show-toplevel``,
which under a bare ``GIT_DIR`` names the current directory whatever repository
is written). ``scripts/test-check-release-staleness.py`` holds it.

The net under every call site is in ``scripts/run-checker.sh`` ("A gate must
not change the repository it judges"): with ``CHECKER_REPO_GUARD`` set, as the
push hook sets it, a gate that moves this worktree's HEAD, changes its
index's content or rewrites the shared config stops the run by name,
whatever it reported.

What to use
-----------

``clean_env()`` for a subprocess that should pick its repository by ``cwd`` or
``-C``::

    subprocess.run(["git", "-C", tmp, "commit", ...], env=gitenv.clean_env())

``scrub_environ()`` once at start-up for a standalone *test harness*, which
should never touch the ambient repository at all. It covers every child at
once, including non-git children (a `bash` that runs git carries the
environment onward), and cannot be forgotten at one call site out of twelve::

    import gitenv; gitenv.scrub_environ()

Both keep everything that does not bind a repository -- ``PATH``, ``HOME``,
``GIT_EXEC_PATH``, ``GIT_SSH``, ``GIT_TRACE``, proxy settings -- because the
goal is to choose the repository, not to run git in a vacuum. A blanket "delete
every ``GIT_*``" would also remove ``GIT_EXEC_PATH``, which on some installs is
the only thing telling git where its own subcommands live.
"""

from __future__ import annotations

import contextlib
import os
import shutil
import stat
import sys
import time

# Variables that redirect git to a specific repository, index, object store or
# config file. Everything here is documented in git(1) "ENVIRONMENT VARIABLES"
# under the repository/index/object headings, plus the two that hooks see.
REPO_BINDING_VARS = frozenset({
    # Where the repository is.
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_COMMON_DIR",
    "GIT_NAMESPACE",
    "GIT_PREFIX",
    # Where discovery is allowed to look, which can make an unrelated parent
    # repository visible or hide the intended one.
    "GIT_CEILING_DIRECTORIES",
    "GIT_DISCOVERY_ACROSS_FILESYSTEM",
    # Which index is written. `git add` in a fixture must not write here.
    "GIT_INDEX_FILE",
    "GIT_INDEX_VERSION",
    # Where objects are read and written. `GIT_QUARANTINE_PATH` is set for
    # `pre-receive`/`update` hooks and makes writes land somewhere temporary.
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_QUARANTINE_PATH",
    # Which configuration applies. `GIT_CONFIG_*` can carry `core.bare`,
    # `diff.renames` and anything else a fixture is trying to control itself.
    "GIT_CONFIG",
    "GIT_CONFIG_GLOBAL",
    "GIT_CONFIG_SYSTEM",
    "GIT_CONFIG_NOSYSTEM",
    "GIT_CONFIG_COUNT",
    "GIT_CONFIG_PARAMETERS",
})

# `GIT_CONFIG_COUNT=n` is accompanied by `GIT_CONFIG_KEY_0..n-1` and
# `GIT_CONFIG_VALUE_0..n-1`, so dropping the count alone leaves the pairs.
REPO_BINDING_PREFIXES = ("GIT_CONFIG_KEY_", "GIT_CONFIG_VALUE_")


def binds_a_repository(name: str) -> bool:
    """Whether this environment variable would override `cwd` / `-C`."""
    return name in REPO_BINDING_VARS or name.startswith(REPO_BINDING_PREFIXES)


def _for_a_child():
    """Tell the gate cache, if it is tracing this process, that the environment
    is being copied for a child rather than read.

    `scripts/gate-cache.py` treats a walk over the whole environment as reading
    all of it, which would make every gate that calls `clean_env` uncacheable
    (and every per-session variable an input). The copy made here only ever
    reaches a child's environment: a git child's relevant variables are
    recorded where it starts, and a Python child's reads are traced. Looked up
    through `sys.modules`, so nothing is imported when no tracer is loaded.
    """
    tracer = sys.modules.get("gatecache_trace")
    return tracer.child_env_copy() if tracer is not None else contextlib.nullcontext()


def clean_env(base: dict[str, str] | None = None) -> dict[str, str]:
    """A copy of `base` (default `os.environ`) with those bindings removed."""
    with _for_a_child():
        env = dict(os.environ if base is None else base)
    for name in [n for n in env if binds_a_repository(n)]:
        del env[name]
    return env


def scrub_environ() -> list[str]:
    """Remove the bindings from this process, so every child inherits none.

    Returns the names that were removed, which is worth printing when a test
    harness wants to say why it ignored the environment it was handed.
    """
    with _for_a_child():
        removed = [n for n in os.environ if binds_a_repository(n)]
    for name in removed:
        del os.environ[name]
    return removed


def _writable_and_again(func, path, exc: BaseException) -> None:
    """`shutil.rmtree`'s error handler for `remove_tree`.

    A read-only file: clear the bit and do the failed step again. git makes
    every object and pack file read-only, and Windows will not unlink a
    read-only file, so without this the first object stops the removal. Gone
    already: nothing to do. Anything else is a real failure, raised.
    """
    if isinstance(exc, FileNotFoundError):
        return
    if not isinstance(exc, PermissionError):
        raise exc
    os.chmod(path, os.lstat(path).st_mode | stat.S_IWRITE)
    func(path)


def remove_tree(path: str | os.PathLike[str], attempts: int = 5, backoff: float = 0.05) -> None:
    """Remove a throwaway repository -- or any fixture directory -- whole.

    `shutil.rmtree(path, ignore_errors=True)`, which the fixtures here used,
    fails silently on the first of git's read-only object files on Windows and
    leaves the whole fixture behind: 43 of `test-gate-cache.py`'s were found
    in the temp directory on 2026-10-02, and `test-boot-history-commit.py`
    said "[WinError 5] Access is denied" on every run. This clears the bit
    and carries on, and retries the whole removal a few times with a short
    backoff, because Windows keeps a handle on a just-written file for a
    moment (the indexer, Defender) and a directory cannot go while one is
    held. What still cannot be removed is raised: a fixture left behind is a
    leak worth hearing about. A tree that is not there is not an error.
    """
    for attempt in range(1, attempts + 1):
        try:
            if sys.version_info >= (3, 12):
                shutil.rmtree(path, onexc=_writable_and_again)
            else:
                shutil.rmtree(path, onerror=lambda f, p, ei: _writable_and_again(f, p, ei[1]))
            return
        except FileNotFoundError:
            return
        except OSError:
            if attempt == attempts:
                raise
            time.sleep(backoff * attempt)
