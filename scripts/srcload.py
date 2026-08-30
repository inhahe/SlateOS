#!/usr/bin/env python3
"""Import a `scripts/*.py` by path, from its source text, never from bytecode.

Most scripts in this directory are named with hyphens, so they cannot be
imported by name; every test suite here therefore loads its subject by path.
The obvious way to do that is `importlib.util.spec_from_file_location` +
`spec.loader.exec_module`, which is what all fourteen of them used to do, and
which has a defect that makes a test suite lie.

The defect
----------

`SourceFileLoader` consults `__pycache__`. It decides the cached bytecode is
current by comparing the source file's `(mtime, size)` against the pair
recorded in the `.pyc` header -- and the recorded mtime has **one-second**
resolution. So two writes to a script inside the same second, producing the
same byte count, leave the second write invisible: the loader finds a `.pyc`
whose stamp still matches and executes the *previous* version of the file.

That is not theoretical, and it was not found by reading the docs. It was found
by mutation-testing `open-requests.py`: two mutants were reported as having
survived -- the suite printed all-pass against code that was not on disk -- and
both were caught the moment they were re-run one at a time, slowly. The window
is narrow, but the workload that opens it is exactly the workload we care most
about: any harness that rewrites the file under test in a loop, which is what
mutation testing, bisecting and "edit, save, re-run" all are. Same-size rewrites
are the common case there, because a mutant usually swaps one expression for
another of similar length.

A test suite that can silently validate bytes that are not on disk is worse
than no suite, because it reports a green result with the same confidence as a
real one. `compile()` on freshly-read text has no cache to be stale, which is
all this module is.

The bootstrap caveat, stated plainly
------------------------------------

This module is itself imported by name, through the ordinary machinery, so
*it* can be served from a stale `.pyc` -- the defect it exists to fix applies
one level up to the fixer. That is accepted rather than overlooked, for a
reason that is specific and worth stating: the window only opens for a file
that is rewritten twice within one second, and the file a harness rewrites is
the subject under test, never the loader. `srcload.py` changes when someone
edits it by hand, minutes apart. If that ever stops being true -- if something
starts generating this file -- the fix is for its caller to `exec` it, and the
duplication would then be justified.

Usage::

    sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
    import srcload

    mod = srcload.load(SCRIPT)                  # name derived from the path
    mod = srcload.load(SCRIPT, "boot_history")  # or given explicitly
"""

from __future__ import annotations

import os
import sys
import types

__all__ = ["load", "module_name_for"]


def module_name_for(path: str) -> str:
    """`scripts/boot-history.py` -> `boot_history`.

    The derived name is what the module sees as `__name__` and the key it is
    registered under, so it has to be a legal identifier: hyphens and dots are
    the only characters our script names actually use that are not, but the
    substitution is deliberately not narrowed to those two, because a name that
    is *nearly* an identifier fails later and less legibly than one that is
    obviously wrong.
    """
    stem = os.path.basename(path).removesuffix(".py")
    cleaned = "".join(c if c.isalnum() or c == "_" else "_" for c in stem)
    if not cleaned or cleaned[0].isdigit():
        cleaned = f"_{cleaned}"
    return cleaned


def load(path: str, name: str | None = None) -> types.ModuleType:
    """Execute `path` as a module and return it. Always reads the file.

    Registered in `sys.modules` **before** the body runs, and that ordering is
    load-bearing rather than tidy: `@dataclass` resolves the defining module by
    looking up `sys.modules[cls.__module__]` while the class body is still
    executing. Skipping the registration makes that lookup return `None` and
    raises a bare `'NoneType' object has no attribute '__dict__'` from inside
    `dataclasses.py`, which names neither the module nor the cause. Four suites
    here had discovered that independently and worked around it locally.

    A module whose body raises is removed again, so a caller that catches the
    error cannot go on to find a half-initialised module under its name -- the
    same thing the real import machinery does, and for the same reason.
    """
    if name is None:
        name = module_name_for(path)
    with open(path, encoding="utf-8") as fh:
        source = fh.read()
    module = types.ModuleType(name)
    module.__file__ = os.path.abspath(path)
    sys.modules[name] = module
    try:
        exec(compile(source, path, "exec"), module.__dict__)  # noqa: S102
    except BaseException:
        sys.modules.pop(name, None)
        raise
    return module
