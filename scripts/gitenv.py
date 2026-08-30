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

import os

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


def clean_env(base: dict[str, str] | None = None) -> dict[str, str]:
    """A copy of `base` (default `os.environ`) with those bindings removed."""
    env = dict(os.environ if base is None else base)
    for name in [n for n in env if binds_a_repository(n)]:
        del env[name]
    return env


def scrub_environ() -> list[str]:
    """Remove the bindings from this process, so every child inherits none.

    Returns the names that were removed, which is worth printing when a test
    harness wants to say why it ignored the environment it was handed.
    """
    removed = [n for n in os.environ if binds_a_repository(n)]
    for name in removed:
        del os.environ[name]
    return removed
