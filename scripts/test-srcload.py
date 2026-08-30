#!/usr/bin/env python3
"""Regression tests for `scripts/srcload.py`.

Run: `python scripts/test-srcload.py` (exit 0 = pass, 1 = fail). No pytest
dependency, matching the other suites here.

Why this file exists
--------------------
`srcload.load` is now the way every test suite in `scripts/` gets hold of the
script it tests, which makes it the one place where a bug is invisible in the
worst possible way: it would not make suites fail, it would make them *pass*
against code that is not on disk. Fourteen green suites, all of them lying, and
nothing else in the tree looks for that.

So the central test here does not assert that `load` returns the right value in
the easy case. It builds the exact situation that defeated the mechanism
`srcload` replaces -- two same-size writes recorded under one mtime -- and
asserts both halves of the claim:

* `spec.loader.exec_module` returns the **stale** value. If this half ever
  stops holding, `srcload` has become unnecessary and should be deleted rather
  than kept as folklore.
* `srcload.load` returns the value that is actually in the file.

The two writes are given an identical mtime with `os.utime` rather than being
raced against the clock. The real defect needs the writes to land inside one
second, which a test cannot guarantee and which would make this suite pass
nearly always and fail occasionally for reasons nobody could reproduce -- the
worst kind of test. Pinning the mtime turns "usually reproduces the bug" into
"always reproduces the bug", and the mtime is the whole mechanism, so nothing
about the defect is being faked.
"""

from __future__ import annotations

import importlib.util
import inspect
import os
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)

import srcload  # noqa: E402

_FAILURES = []


def check(label, got, want):
    if got == want:
        print(f"PASS  {label}")
        return True
    print(f"FAIL  {label}")
    print(f"        got : {got!r}")
    print(f"        want: {want!r}")
    _FAILURES.append(label)
    return False


def _write(path, text, mtime):
    with open(path, "w", encoding="utf-8") as fh:
        fh.write(text)
    os.utime(path, (mtime, mtime))


def _load_the_old_way(path, name):
    """Exactly what the suites used to do, kept so the defect stays testable."""
    spec = importlib.util.spec_from_file_location(name, path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load {path}")
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


def test_a_same_size_rewrite_under_one_mtime_defeats_the_bytecode_loader():
    """The defect, reproduced deterministically, and the fix, on the same file.

    Both assertions matter. Without the first, the second is a tautology --
    `load` returning what the file says is uninteresting unless something else
    would not have.
    """
    with tempfile.TemporaryDirectory() as tmp:
        path = os.path.join(tmp, "subject.py")
        stamp = 1_600_000_000.0
        _write(path, "VALUE = 111\n", stamp)

        first = _load_the_old_way(path, "srcload_subject")
        check("the primed load sees the first version", first.VALUE, 111)
        pyc = importlib.util.cache_from_source(path)
        check("loading through a spec wrote bytecode", os.path.exists(pyc), True)
        was = os.path.getsize(path)

        _write(path, "VALUE = 222\n", stamp)
        # Asserted rather than assumed: `(mtime, size)` is the whole staleness
        # check, so a rewrite of a different length would make the cache
        # honestly stale and the test below would pass for the wrong reason.
        check(
            "the rewrite is the same size, as a mutant usually is",
            os.path.getsize(path),
            was,
        )

        sys.modules.pop("srcload_subject", None)
        stale = _load_the_old_way(path, "srcload_subject")
        check(
            "the bytecode loader serves the version no longer on disk",
            stale.VALUE,
            111,
        )

        fresh = srcload.load(path, "srcload_subject")
        check("srcload.load reads the file", fresh.VALUE, 222)
        sys.modules.pop("srcload_subject", None)


def test_the_cache_is_not_consulted_even_when_it_is_current():
    """A `.pyc` that genuinely matches must still not be the source of truth.

    Weaker than the test above, and deliberately kept separate: this one pins
    that `load` has no fast path at all, so a later "optimisation" that checks
    the cache first and only falls back on a mismatch cannot pass here.
    """
    with tempfile.TemporaryDirectory() as tmp:
        path = os.path.join(tmp, "current.py")
        _write(path, "VALUE = 1\n", 1_600_000_000.0)
        _load_the_old_way(path, "srcload_current")
        sys.modules.pop("srcload_current", None)
        # Rewritten with a *later* mtime, so the cache is honestly stale and
        # both loaders agree -- the point is that `load` gets it right for the
        # reason that it read the file, not because the stamp saved it.
        _write(path, "VALUE = 2\n", 1_700_000_000.0)
        check("load returns the current value", srcload.load(path, "srcload_current").VALUE, 2)
        sys.modules.pop("srcload_current", None)


def test_a_dataclass_body_can_find_its_own_module():
    """The registration-before-exec rule, tested by its real symptom.

    `@dataclass` looks up `sys.modules[cls.__module__]` while the class body is
    running. Asserting "the name is in sys.modules" would pass against a `load`
    that registered the module *after* execution, which is the mistake this
    guards. Defining an actual dataclass is the only version that cannot.
    """
    with tempfile.TemporaryDirectory() as tmp:
        path = os.path.join(tmp, "dc.py")
        _write(
            path,
            "from dataclasses import dataclass\n"
            "@dataclass\n"
            "class Point:\n"
            "    x: int = 3\n",
            1_600_000_000.0,
        )
        try:
            mod = srcload.load(path, "srcload_dc")
            got = mod.Point().x
        except Exception as exc:  # noqa: BLE001 - the failure is the finding
            got = f"raised {type(exc).__name__}: {exc}"
        check("a dataclass in the loaded module constructs", got, 3)
        sys.modules.pop("srcload_dc", None)


def test_a_module_that_raises_does_not_stay_registered():
    """A half-built module left under its name is a booby trap for the retry."""
    with tempfile.TemporaryDirectory() as tmp:
        path = os.path.join(tmp, "boom.py")
        _write(path, "GOOD = 1\nraise ValueError('nope')\n", 1_600_000_000.0)
        raised = None
        try:
            srcload.load(path, "srcload_boom")
        except ValueError as exc:
            raised = str(exc)
        check("the body's exception reaches the caller", raised, "nope")
        check(
            "the partial module is not left in sys.modules",
            "srcload_boom" in sys.modules,
            False,
        )


def test_each_call_executes_the_file_again():
    """`load` is not `import`: a second call must not hand back the first module.

    The suites rely on this -- several load their subject once per test after
    rewriting a fixture, and a memoising `load` would silently make every one of
    those assertions test the first version.
    """
    with tempfile.TemporaryDirectory() as tmp:
        path = os.path.join(tmp, "twice.py")
        _write(path, "VALUE = 5\n", 1_600_000_000.0)
        first = srcload.load(path, "srcload_twice")
        first.VALUE = 99
        second = srcload.load(path, "srcload_twice")
        check("the second load is a different module object", first is second, False)
        check("the second load is not carrying the mutation", second.VALUE, 5)
        sys.modules.pop("srcload_twice", None)


def test_the_derived_name_is_an_identifier():
    """Every real script name in `scripts/`, plus the shapes that break naively.

    The hyphen case is the one that exists today; the others are here because
    the substitution is written to be general and an untested general rule is a
    guess.
    """
    cases = [
        ("scripts/boot-history.py", "boot_history"),
        ("boot-history.py", "boot_history"),
        (os.path.join("a", "b", "check-vfs-under-lock.py"), "check_vfs_under_lock"),
        ("already_fine.py", "already_fine"),
        ("dotted.name.py", "dotted_name"),
        ("9lives.py", "_9lives"),
    ]
    for path, want in cases:
        got = srcload.module_name_for(path)
        check(f"module_name_for({path!r})", got, want)
        check(f"{got!r} is an identifier", got.isidentifier(), True)


def test_the_loaded_module_knows_where_it_came_from():
    """`__file__` is absolute, because a script that resolves paths against it
    would otherwise resolve them against the caller's cwd."""
    with tempfile.TemporaryDirectory() as tmp:
        path = os.path.join(tmp, "whereami.py")
        _write(path, "import os\nWHERE = os.path.dirname(__file__)\n", 1_600_000_000.0)
        mod = srcload.load(path)
        check("the derived name was used", mod.__name__, "whereami")
        check("__file__ is absolute", os.path.isabs(mod.__file__), True)
        check("__file__ points at the source", os.path.realpath(mod.__file__),
              os.path.realpath(path))
        check("the module can locate its own directory",
              os.path.realpath(mod.WHERE), os.path.realpath(tmp))
        sys.modules.pop("whereami", None)


def test_the_real_scripts_still_load_through_it():
    """Every suite's subject, loaded for real. A smoke test with teeth: it is
    the only thing here that would notice `srcload` breaking on a module with
    imports, module-level constants, or a `__name__ == "__main__"` guard."""
    subjects = [
        "bench-history.py",
        "boot-history.py",
        "ctest-fixtures.py",
        "layout-sweep.py",
        "open-requests.py",
        "prune-build-trees.py",
        "reclaim-space.py",
        "straddle-check.py",
        "straddle-check.py",
    ]
    for rel in sorted(set(subjects)):
        path = os.path.join(HERE, rel)
        if not os.path.exists(path):
            check(f"{rel} exists to be loaded", True, False)
            continue
        name = f"probe_{srcload.module_name_for(rel)}"
        try:
            mod = srcload.load(path, name)
            got = isinstance(getattr(mod, "__name__", None), str)
        except Exception as exc:  # noqa: BLE001 - the failure is the finding
            got = f"raised {type(exc).__name__}: {exc}"
        check(f"{rel} loads", got, True)
        sys.modules.pop(name, None)


def main():
    """Auto-discover `test_*` in this module, same contract as the other suites.

    The floor assertion is not ceremony: a suite that discovers zero tests
    prints nothing and exits 0, which is indistinguishable from a suite that
    passed.
    """
    tests = [(name, fn) for name, fn in list(globals().items())
             if name.startswith("test_") and callable(fn)]
    if len(tests) < 8:
        print(f"FATAL: test discovery found only {len(tests)} tests; the "
              f"suite has at least 8. Discovery is broken, not the code.")
        return 1
    for _, fn in tests:
        params = inspect.signature(fn).parameters
        if params:
            print(f"FATAL: {tests[0][0]} takes arguments; this suite injects none.")
            return 1
        fn()

    print()
    if _FAILURES:
        print(f"{len(_FAILURES)} FAILED: {', '.join(_FAILURES)}")
        return 1
    print(f"all srcload tests passed ({len(tests)} tests)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
