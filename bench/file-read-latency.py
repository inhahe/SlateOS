#!/usr/bin/env python3
"""Measure per-file open+read latency across one or more trees.

Why this exists
---------------
`open-questions.md` A-Q7 (2026-09-03) asserted that every build on this host
paid "~70 ms per file opened", and concluded from one measurement -- "a second
full pass over all 805 files, immediately: still 61.8 s" -- that a warm page
cache changed nothing, and therefore that the cost was neither the disk nor
the cache but per-open inspection by the antivirus.

That conclusion asked the operator to weaken a system-wide security setting.
It was wrong, and it went unchallenged for six days because there was no way
to re-run it: the numbers lived in prose, not in a script. The warm second
pass over the same files on the same drive costs ~0.19 s, not 61.8 s.

So this file is the claim, in runnable form. A performance assertion about
this tree should be reproducible by running something, not by trusting a
paragraph.

Cache state is the whole game here
----------------------------------
The first pass over a tree measures whatever state the cache happens to be in
("as-found"); every later pass is warm. Windows offers no unprivileged way to
drop the file cache, so a *genuine* cold number requires files the machine has
not touched recently -- in practice a tree just written, or a reboot. This
script therefore reports each pass separately and never averages a cold pass
together with warm ones: the spread between them is the measurement, not noise
to be smoothed away.

Usage
-----
    python bench/file-read-latency.py                       # this tree
    python bench/file-read-latency.py --roots A=. B=D:/old  # compare trees
    python bench/file-read-latency.py --self-test
"""

from __future__ import annotations

import argparse
import os
import statistics
import sys
import time

DEFAULT_EXT = ".rs"
DEFAULT_PASSES = 3


def collect(root: str, ext: str) -> list[str]:
    """Return a sorted list of files under `root` whose name ends with `ext`.

    Sorted so that two trees with identical content are walked in the same
    order, which keeps a comparison between them honest.
    """
    found: list[str] = []
    for dirpath, _dirs, files in os.walk(root):
        for name in files:
            if name.endswith(ext):
                found.append(os.path.join(dirpath, name))
    found.sort()
    return found


def percentile(sorted_values: list[float], fraction: float) -> float:
    """Nearest-rank percentile of an already-sorted, non-empty list."""
    if not sorted_values:
        raise ValueError("percentile of an empty sequence")
    index = int(len(sorted_values) * fraction)
    if index >= len(sorted_values):
        index = len(sorted_values) - 1
    return sorted_values[index]


def read_pass(paths: list[str]) -> tuple[list[float], int, int]:
    """Open and fully read each path, timing each file individually.

    Returns (milliseconds-per-file, total bytes, files that could not be read).
    The whole file is read, not merely opened, because that is the operation a
    build or a checker actually performs -- timing a bare open() measures a
    different thing and is not comparable to A-Q7's original numbers.
    """
    timings: list[float] = []
    total_bytes = 0
    failures = 0
    for path in paths:
        start = time.perf_counter_ns()
        try:
            with open(path, "rb") as handle:
                data = handle.read()
        except OSError:
            # Counted and reported rather than discarded: a tree that half
            # disappears mid-run would otherwise look merely fast.
            failures += 1
            continue
        end = time.perf_counter_ns()
        total_bytes += len(data)
        timings.append((end - start) / 1e6)
    return timings, total_bytes, failures


def describe(label: str, index: int, timings: list[float],
             total_bytes: int, failures: int) -> str:
    """One line per pass. `index` is 0-based; pass 1 is the as-found one."""
    if not timings:
        return f"pass {index + 1}  {label}  no readable files"
    ordered = sorted(timings)
    state = "as-found" if index == 0 else "warm"
    note = f"  [{failures} unreadable]" if failures else ""
    return (
        f"pass {index + 1}  {label:<10} {state:<8} n={len(timings):4d}  "
        f"total={sum(timings) / 1000.0:7.3f}s  "
        f"median={statistics.median(timings):7.3f}ms  "
        f"mean={statistics.fmean(timings):7.3f}ms  "
        f"p95={percentile(ordered, 0.95):7.3f}ms  "
        f"max={max(timings):8.2f}ms  ({total_bytes / 1e6:.1f} MB){note}"
    )


def run(roots: dict[str, str], ext: str, passes: int) -> int:
    """Measure every root for `passes` passes, alternating order between them.

    Alternating matters: whichever tree is read first in a pass warms the cache
    and loads the disk queue for the one after it, so a fixed order would hand
    one tree a systematic advantage and the comparison would measure the order
    rather than the trees.
    """
    files: dict[str, list[str]] = {}
    for label, path in roots.items():
        if not os.path.isdir(path):
            print(f"error: {label}: not a directory: {path}", file=sys.stderr)
            return 2
        files[label] = collect(path, ext)
        print(f"{label}: {len(files[label])} *{ext} files under {path}")
        if not files[label]:
            print(f"error: {label}: nothing to measure", file=sys.stderr)
            return 2
    print()

    labels = list(roots)
    for index in range(passes):
        order = labels if index % 2 == 0 else list(reversed(labels))
        for label in order:
            timings, total_bytes, failures = read_pass(files[label])
            print(describe(label, index, timings, total_bytes, failures))
        print()

    if len(labels) > 1:
        counts = {len(files[label]) for label in labels}
        if len(counts) > 1:
            spread = ", ".join(f"{k}={len(files[k])}" for k in labels)
            print(f"note: trees hold different file counts ({spread}); "
                  "totals are not directly comparable.")
    return 0


def self_test() -> int:
    """Check the pure helpers.

    The timing loop itself is not asserted on -- a wall-clock number has no
    correct value to compare against.
    """
    failures = 0

    def check(name: str, got: object, want: object) -> None:
        nonlocal failures
        if got != want:
            print(f"  FAIL {name}: got {got!r}, want {want!r}")
            failures += 1
        else:
            print(f"  ok   {name}")

    ordered = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0]
    check("percentile p95 of 10", percentile(ordered, 0.95), 10.0)
    check("percentile p50 of 10", percentile(ordered, 0.50), 6.0)
    check("percentile p0  of 10", percentile(ordered, 0.0), 1.0)
    # Nearest-rank must not index past the end for fraction 1.0.
    check("percentile p100 clamps", percentile(ordered, 1.0), 10.0)
    check("percentile single", percentile([42.0], 0.95), 42.0)

    try:
        percentile([], 0.5)
    except ValueError:
        print("  ok   percentile([]) raises")
    else:
        print("  FAIL percentile([]) did not raise")
        failures += 1

    # A pass over a real, known file must produce exactly one timing and the
    # file's true byte count -- this is what catches a silently-skipped read.
    here = os.path.abspath(__file__)
    timings, total_bytes, bad = read_pass([here])
    check("read_pass timings length", len(timings), 1)
    check("read_pass byte count", total_bytes, os.path.getsize(here))
    check("read_pass failures", bad, 0)

    # A path that does not exist must be counted, not silently dropped.
    timings, total_bytes, bad = read_pass([here + ".does-not-exist"])
    check("missing file -> no timing", len(timings), 0)
    check("missing file -> counted", bad, 1)

    # Pass 1 is labelled as-found; later passes warm. A regression here would
    # let a warm number be reported as a cold one -- exactly the error that
    # made A-Q7 wrong.
    check("pass 1 labelled as-found",
          "as-found" in describe("X", 0, [1.0], 10, 0), True)
    check("pass 2 labelled warm",
          "warm" in describe("X", 1, [1.0], 10, 0), True)
    check("unreadable files surface in output",
          "[2 unreadable]" in describe("X", 0, [1.0], 10, 2), True)

    # parse_roots must reject the shapes that would silently measure one tree
    # twice, or measure nothing.
    for bad_arg in ("noequals", "=/tmp", "label="):
        try:
            parse_roots([bad_arg])
        except argparse.ArgumentTypeError:
            print(f"  ok   parse_roots rejects {bad_arg!r}")
        else:
            print(f"  FAIL parse_roots accepted {bad_arg!r}")
            failures += 1
    try:
        parse_roots(["A=/one", "A=/two"])
    except argparse.ArgumentTypeError:
        print("  ok   parse_roots rejects a duplicate label")
    else:
        print("  FAIL parse_roots accepted a duplicate label")
        failures += 1
    check("parse_roots keeps order",
          list(parse_roots(["B=/b", "A=/a"])), ["B", "A"])

    verdict = "FAILED" if failures else "OK"
    print(f"\nfile-read-latency self-test: {verdict} "
          f"({failures} failure(s))")
    return 1 if failures else 0


def parse_roots(pairs: list[str]) -> dict[str, str]:
    """Parse `LABEL=PATH` arguments, preserving order."""
    roots: dict[str, str] = {}
    for pair in pairs:
        label, sep, path = pair.partition("=")
        if not sep or not label or not path:
            raise argparse.ArgumentTypeError(
                f"expected LABEL=PATH, got {pair!r}")
        if label in roots:
            raise argparse.ArgumentTypeError(f"duplicate label {label!r}")
        roots[label] = path
    return roots


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(
        description="Measure per-file open+read latency across trees.")
    parser.add_argument("--roots", nargs="+", metavar="LABEL=PATH",
                        help="trees to compare (default: kernel/src here)")
    parser.add_argument("--ext", default=DEFAULT_EXT,
                        help=f"file extension to walk (default: {DEFAULT_EXT})")
    parser.add_argument("--passes", type=int, default=DEFAULT_PASSES,
                        help=f"passes per tree (default: {DEFAULT_PASSES})")
    parser.add_argument("--self-test", action="store_true",
                        help="check the helpers and exit")
    args = parser.parse_args(argv)

    if args.self_test:
        return self_test()

    if args.passes < 1:
        print("error: --passes must be at least 1", file=sys.stderr)
        return 2

    if args.roots:
        try:
            roots = parse_roots(args.roots)
        except argparse.ArgumentTypeError as exc:
            print(f"error: {exc}", file=sys.stderr)
            return 2
    else:
        here = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
        roots = {"this-tree": os.path.join(here, "kernel", "src")}

    return run(roots, args.ext, args.passes)


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
