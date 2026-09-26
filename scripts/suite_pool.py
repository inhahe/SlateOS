#!/usr/bin/env python3
"""Run a tooling suite's independent cases a few at a time, with one-at-a-time output.

Why this exists
---------------

Every boot test runs the tooling's own suites (`scripts/test-*.py`) before it
builds anything, and on 2026-09-25 two of them took 83 minutes of one lane's
3.2-hour gate phase: `test-checkers-honour-head.py` 3127 s and
`test-pre-push-fmt-gate.py` 1894 s. Neither is computing anything. Each case
builds a throwaway git repository and runs a checker, or the whole pre-push
hook, against it -- process start-up and `git`, one case after another, on a
machine where six lanes share twelve cores. The cases share nothing with each
other, so running a few at once is the cheap half of the remedy
(`known-issues.md` -> `TD-C-TWO-TOOLING-SUITES-TAKE-EIGHTY-MINUTES-OF-EVERY-BOOT`).

What it keeps
-------------

A suite's log is read by people and by the boot test's own summary, so a run
here prints exactly what a one-at-a-time run would, in the same order: each
case's prints go to a buffer of its own while it runs, and the buffers are
written out in the list's order as the cases finish. A heading printed before
a group of cases is the first case's heading, so it lands in the same place. A
case that raises stops the suite with its traceback, as the plain loop did,
after its own output.

`SUITE_JOBS=1` restores the plain loop exactly. The default is a third of the
logical cores, at most four: the boot test runs these suites beside other
lanes' builds, and a pool that took every core would slow them as much as it
sped this up.

Per-case state a suite used to keep in a module global -- a label suffix, say
-- belongs in `context`, a thread-local: set it inside the case's callable,
where it is that case's alone.

A case's directory is removed after it with retries (`_remove`): on Windows a
process the case ran -- a `git` -- can hold a file in it for a moment after
exiting, and a clean-up that failed once used to fail the suite, and with it a
boot's whole gate phase, over a case that had passed (2026-09-26).
"""

from __future__ import annotations

import concurrent.futures
import contextlib
import io
import os
import shutil
import stat
import sys
import tempfile
import threading
import time
from typing import Callable, Iterator, Optional, Sequence

#: Per-case state. Each case runs on one worker thread from start to finish,
#: so anything a case sets here is that case's and nobody else's.
context = threading.local()


class _CaseOutput(io.TextIOBase):
    """`sys.stdout`/`sys.stderr` while cases run: the running case's buffer on
    a worker thread, the real stream everywhere else."""

    def __init__(self, real) -> None:
        self._real = real

    def write(self, text: str) -> int:
        buf = getattr(context, "_out", None)
        return (buf if buf is not None else self._real).write(text)

    def flush(self) -> None:
        self._real.flush()


#: How many times a case's directory is tried before it is left behind, and
#: the pause before the first retry; each later pause is one step longer.
REMOVE_ATTEMPTS = 8
REMOVE_STEP_SECONDS = 0.1


def _writable_then_again(function, path, _error) -> None:
    """`shutil.rmtree`'s error handler: make `path` writable and try again.

    `git` writes its objects read-only, and Windows will not delete a
    read-only file -- which `tempfile.TemporaryDirectory` handles the same
    way. A second failure propagates, and `_remove` retries the whole tree.
    """
    os.chmod(path, stat.S_IWRITE)
    function(path)


def _remove(path: str, attempts: int = REMOVE_ATTEMPTS) -> bool:
    """Remove the tree at `path`, trying `attempts` times with a growing
    pause; answer whether it is gone.

    A tree that cannot be removed is left, with a line on stderr naming it:
    it is a temporary directory, which the system cleans, and failing a case
    that passed over its clean-up is the wrong answer to "is the checker
    right". Nothing it leaves is in the tree being tested.
    """
    handler = ({"onexc": _writable_then_again} if sys.version_info >= (3, 12)
               else {"onerror": _writable_then_again})
    for attempt in range(max(1, attempts)):
        try:
            shutil.rmtree(path, **handler)
            return True
        except FileNotFoundError:
            return True
        except OSError:
            if attempt + 1 < attempts:
                time.sleep(REMOVE_STEP_SECONDS * (attempt + 1))
    if not os.path.exists(path):
        return True
    sys.__stderr__.write(
        f"suite_pool: left {path} behind: something still holds a file in it\n")
    return False


@contextlib.contextmanager
def _case_dir() -> Iterator[str]:
    """A temporary directory for one case, removed by `_remove` after it."""
    path = tempfile.mkdtemp()
    try:
        yield path
    finally:
        _remove(path)


def jobs() -> int:
    """How many cases run at once: `SUITE_JOBS` if set, else a third of the
    logical cores, at most four, and never fewer than one."""
    raw = os.environ.get("SUITE_JOBS")
    if raw is not None:
        try:
            return max(1, int(raw))
        except ValueError:
            return 1
    return max(1, min(4, (os.cpu_count() or 1) // 3))


def run(cases: Sequence[tuple[Optional[str], Callable[[str], None]]],
        workers: Optional[int] = None) -> None:
    """Run each case's callable, `workers` at a time (default: [`jobs`]).

    Each callable is handed a temporary directory of its own, removed when it
    returns. Its output is printed in the list's order, after its heading if
    the entry has one. With one worker this is the plain loop, run on the
    calling thread with nothing buffered.
    """
    count = jobs() if workers is None else max(1, workers)
    if count == 1:
        for heading, case in cases:
            if heading:
                print(heading)
            with _case_dir() as tmp:
                case(tmp)
        return

    def one(case: Callable[[str], None]):
        buf = io.StringIO()
        context._out = buf
        try:
            with _case_dir() as tmp:
                case(tmp)
        except BaseException as error:  # re-raised on the calling thread
            return buf.getvalue(), error
        finally:
            context._out = None
        return buf.getvalue(), None

    real_out, real_err = sys.stdout, sys.stderr
    sys.stdout, sys.stderr = _CaseOutput(real_out), _CaseOutput(real_err)
    try:
        with concurrent.futures.ThreadPoolExecutor(max_workers=count) as pool:
            futures = [(heading, pool.submit(one, case)) for heading, case in cases]
            for heading, future in futures:
                text, error = future.result()
                if heading:
                    real_out.write(heading + "\n")
                real_out.write(text)
                real_out.flush()
                if error is not None:
                    raise error
    finally:
        sys.stdout, sys.stderr = real_out, real_err


def self_test() -> int:
    """Order, isolation and failure, with real threads."""
    import time

    problems: list[str] = []
    seen_dirs: list[str] = []

    def case(n: int) -> Callable[[str], None]:
        def body(tmp: str) -> None:
            seen_dirs.append(tmp)
            context.label = f"#{n}"
            # The later cases finish first, which is what shows the order.
            time.sleep(0.05 * (4 - n))
            print(f"case {n} {context.label}")
        return body

    captured = io.StringIO()
    saved = sys.stdout
    sys.stdout = captured
    try:
        run([(f"-- {n}" if n == 0 else None, case(n)) for n in range(4)], workers=4)
    finally:
        sys.stdout = saved
    want = "-- 0\ncase 0 #0\ncase 1 #1\ncase 2 #2\ncase 3 #3\n"
    if captured.getvalue() != want:
        problems.append(f"output out of order or mixed: {captured.getvalue()!r}")
    if len(set(seen_dirs)) != 4:
        problems.append(f"cases shared a directory: {seen_dirs}")

    def boom(_tmp: str) -> None:
        print("before the error")
        raise ValueError("boom")

    captured = io.StringIO()
    sys.stdout = captured
    try:
        run([(None, boom)], workers=2)
        problems.append("a case that raised did not stop the run")
    except ValueError:
        if "before the error" not in captured.getvalue():
            problems.append("the failing case's own output was lost")
    finally:
        sys.stdout = saved

    # Every case's directory is gone afterwards -- a read-only file in it,
    # as `git` writes its objects, included.
    def read_only(tmp: str) -> None:
        name = os.path.join(tmp, "object")
        with open(name, "w", encoding="utf-8") as handle:
            handle.write("x")
        os.chmod(name, stat.S_IREAD)

    kept: list[str] = []
    run([(None, lambda tmp: (kept.append(tmp), read_only(tmp))[-1])], workers=1)
    run([(None, lambda tmp: (kept.append(tmp), read_only(tmp))[-1])], workers=2)
    for tmp in kept:
        if os.path.exists(tmp):
            problems.append(f"a case's directory was left: {tmp}")

    # A removal that fails a few times, as a file a just-exited process holds
    # makes it fail on Windows, is retried until it works...
    real_rmtree = shutil.rmtree
    failures = {"left": 3}

    def flaky(path, **kwargs):
        if failures["left"] > 0:
            failures["left"] -= 1
            raise PermissionError(32, "being used by another process", path)
        return real_rmtree(path, **kwargs)

    target = tempfile.mkdtemp()
    shutil.rmtree = flaky
    try:
        if not _remove(target, attempts=5) or os.path.exists(target):
            problems.append("a removal that failed three times was not retried")
    finally:
        shutil.rmtree = real_rmtree

    # ...and one that never works is left behind without failing anything.
    def stuck(path, **kwargs):
        raise PermissionError(32, "being used by another process", path)

    target = tempfile.mkdtemp()
    shutil.rmtree = stuck
    saved_err = sys.__stderr__
    sys.__stderr__ = io.StringIO()
    try:
        gone = _remove(target, attempts=2)
        said = sys.__stderr__.getvalue()
    except OSError as error:
        gone, said = True, ""
        problems.append(f"a removal that never works raised: {error}")
    finally:
        shutil.rmtree = real_rmtree
        sys.__stderr__ = saved_err
    if gone:
        problems.append("a directory that could not be removed was reported gone")
    elif target not in said:
        problems.append("the directory left behind was not named on stderr")
    real_rmtree(target, ignore_errors=True)

    for line in problems:
        print(f"FAIL  {line}")
    print("suite_pool --self-test: " + ("OK" if not problems else f"{len(problems)} failed"))
    return 1 if problems else 0


if __name__ == "__main__":
    sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
    import selftestflag  # noqa: E402

    if selftestflag.wants_selftest(sys.argv[1:]):
        sys.exit(self_test())
    # A module, not a tool: anything else is a mistake, and saying so beats
    # an exit status of 0 that a caller could take for a passing self-test.
    print("suite_pool: a module for scripts/test-*.py; "
          "the only thing it runs by itself is --self-test", file=sys.stderr)
    sys.exit(2)
