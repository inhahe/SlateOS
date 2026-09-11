#!/usr/bin/env python3
"""Say whether incoming `origin/main` commits land in the part of the tree your
test suite actually covers.

Run this in your lane worktree before merging up:

    python scripts/merge-readiness.py

Why this exists
---------------

`CLAUDE.md` says: fetch and merge `origin/main`, run the full suite, *then* merge
up. The reason is that three lanes land work concurrently, so a suite run against
your branch alone says nothing about the combination you are about to push.

Obeying it literally on every merge is expensive, and the expense is what makes it
get skipped. A boot test is ~25 minutes; `main` moves several times an hour. If
every merge-up required a re-merge and a fresh boot test, each lane would spend its
day re-testing because the other two keep landing work -- and a rule that costs
that much gets quietly dropped, which is how lane A came to push a combination
nobody had tested on 2026-09-11 (`known-issues.md` →
`TD-A-I-MERGED-TO-MAIN-HAVING-TESTED-ONLY-THE-PRE-MERGE-STATE`).

The saving grace is that the lanes are *partitioned by directory*. Most of the
time the incoming commits cannot have invalidated your run, and establishing that
is a `git diff --name-only` and a glob intersection. That is the whole of this
script. It was derived by hand after the fact on 2026-09-11; doing it by hand
beforehand is the thing nobody remembers to do.

What it is NOT
--------------

**Advisory only. It never exits non-zero for a finding and nothing runs it as a
gate.** That is deliberate in both directions:

* Blocking would reintroduce the deadlock above, and a gate three agents want to
  bypass is worse than no gate.
* It does not claim the merge is *verified*. It answers exactly one question --
  "could these commits have invalidated my suite run?" -- and a `no` means your
  run still stands for your own subsystem, not that the merge as a whole has been
  exercised. The same boot test that prompted this script also printed
  ``fixtures are behind the tree``, meaning another lane's userspace binaries were
  not exercised at all. Coverage is always narrower than a green light looks.

Exit codes: `0` always, except `2` for a usage or git error, and the
`--self-test` contract (`0` pass, `1` failure) -- see `run_checker`'s convention.
"""

from __future__ import annotations

import argparse
import subprocess
import sys

# What each lane's own suite speaks for. Deliberately the lane's *write* scope
# from CLAUDE.md rather than "everything the build touches": the boot test
# compiles the whole workspace, so an incoming commit anywhere can break the
# build, but only a change inside your own scope can change the BEHAVIOUR your
# suite asserted. Those two failures want different answers, so they are reported
# separately below rather than merged into one verdict.
LANE_SCOPE: dict[str, tuple[str, ...]] = {
    "A": ("kernel/", "bench/", "toolchain/", "scripts/boot-test.sh"),
    "B": ("posix/", "userspace/", "services/", "init/"),
    "C": ("gui/", "apps/", "net", "pkg/"),
}

# Paths that are nobody's subsystem but everybody's verdict: if an incoming commit
# changes one of these, every lane's gate results were produced by different code
# than the one now on main.
SHARED_MACHINERY = ("scripts/", ".githooks/")


def classify(paths: list[str], lane: str) -> dict[str, list[str]]:
    """Split changed paths into the three buckets that call for different action.

    Pure function over a list of path strings -- this is the part worth testing,
    and `--self-test` tests exactly this. The git plumbing around it is left
    untested on purpose rather than mocked: a mock of `git diff` would assert that
    this script agrees with my model of git, which is not the thing that can be
    wrong here.
    """
    scope = LANE_SCOPE.get(lane, ())
    out: dict[str, list[str]] = {"mine": [], "machinery": [], "elsewhere": []}
    for p in paths:
        norm = p.replace("\\", "/")
        if any(norm.startswith(s) for s in scope):
            out["mine"].append(norm)
        elif any(norm.startswith(s) for s in SHARED_MACHINERY):
            out["machinery"].append(norm)
        else:
            out["elsewhere"].append(norm)
    return out


def git(*args: str) -> str:
    res = subprocess.run(
        ["git", *args], capture_output=True, text=True, encoding="utf-8", errors="replace"
    )
    if res.returncode != 0:
        raise RuntimeError(f"git {' '.join(args)} failed: {res.stderr.strip()}")
    return res.stdout


def current_lane() -> str:
    """Ask the project's own authority rather than re-deriving it.

    `which-lane.py --letter` is the single source of truth for this mapping and it
    exits 2 on an unknown lane. Re-implementing its CLAUDE_CONFIG_DIR logic here
    would create a second copy to drift -- which is the failure this whole
    directory of scripts keeps finding elsewhere.
    """
    res = subprocess.run(
        [sys.executable, "scripts/which-lane.py", "--letter"],
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
    )
    if res.returncode != 0:
        return ""
    return res.stdout.strip().upper()[:1]


def report() -> int:
    lane = current_lane()
    if not lane:
        print("merge-readiness: cannot tell which lane this is; run which-lane.py", file=sys.stderr)
        return 2

    try:
        git("fetch", "origin", "--quiet")
        base = git("merge-base", "HEAD", "origin/main").strip()
        commits = [c for c in git("rev-list", f"{base}..origin/main").splitlines() if c]
        paths = [p for p in git("diff", "--name-only", base, "origin/main").splitlines() if p]
    except RuntimeError as e:
        print(f"merge-readiness: {e}", file=sys.stderr)
        return 2

    if not commits:
        print("merge-readiness: origin/main holds nothing you do not already have.")
        print("  Your suite run covers exactly what you are about to push.")
        return 0

    buckets = classify(paths, lane)
    print(
        f"merge-readiness (lane {lane}): origin/main is {len(commits)} commit(s) "
        f"ahead, touching {len(paths)} file(s)."
    )

    if buckets["mine"]:
        print()
        print(f"  *** {len(buckets['mine'])} file(s) are in LANE {lane}'s OWN SCOPE. ***")
        for p in buckets["mine"][:12]:
            print(f"      {p}")
        if len(buckets["mine"]) > 12:
            print(f"      … and {len(buckets['mine']) - 12} more")
        print()
        print("  Your suite run does NOT cover these. Merge origin/main into this")
        print("  branch and re-run before merging up -- this is the case the rule in")
        print("  CLAUDE.md exists for, and the one time it actually costs something.")
    else:
        print(f"  No incoming file is in lane {lane}'s scope "
              f"({', '.join(LANE_SCOPE.get(lane, ()))}).")
        print("  A completed suite run still speaks for your subsystem: the code it")
        print("  asserted behaviour about is byte-identical after the merge.")

    if buckets["machinery"]:
        print()
        print(f"  {len(buckets['machinery'])} file(s) are shared machinery "
              f"(scripts/, hooks) -- your GATE results were produced by older")
        print("  versions of these. Cheap to settle: re-run the gates, not the boot test.")
        for p in buckets["machinery"][:8]:
            print(f"      {p}")
        if len(buckets["machinery"]) > 8:
            print(f"      … and {len(buckets['machinery']) - 8} more")

    if buckets["elsewhere"]:
        print()
        print(f"  {len(buckets['elsewhere'])} file(s) are in another lane's scope. Their")
        print("  behaviour is not yours to vouch for, and your run did not exercise it.")

    print()
    print("  Advisory only -- this never fails. It answers one question: could the")
    print("  incoming commits have invalidated YOUR run? It does not say the merge")
    print("  as a whole has been exercised, and it usually has not.")
    return 0


def self_test() -> int:
    """Test `classify`, which is the only thing here that can be wrong quietly."""
    cases: list[tuple[str, str, list[str], dict[str, list[str]]]] = [
        (
            "lane A: a kernel file is in scope",
            "A",
            ["kernel/src/fs/overlay.rs"],
            {"mine": ["kernel/src/fs/overlay.rs"], "machinery": [], "elsewhere": []},
        ),
        (
            "lane A: userspace is another lane's",
            "A",
            ["userspace/cgroup/src/main.rs"],
            {"mine": [], "machinery": [], "elsewhere": ["userspace/cgroup/src/main.rs"]},
        ),
        (
            "lane A: a script is shared machinery, not lane scope",
            "A",
            ["scripts/rustlex.py"],
            {"mine": [], "machinery": ["scripts/rustlex.py"], "elsewhere": []},
        ),
        (
            "boot-test.sh is lane A's own, and beats the scripts/ rule",
            "A",
            ["scripts/boot-test.sh"],
            {"mine": ["scripts/boot-test.sh"], "machinery": [], "elsewhere": []},
        ),
        (
            "lane B: userspace is in scope",
            "B",
            ["userspace/oils/src/interp.rs"],
            {"mine": ["userspace/oils/src/interp.rs"], "machinery": [], "elsewhere": []},
        ),
        (
            "lane B: the kernel is not",
            "B",
            ["kernel/src/main.rs"],
            {"mine": [], "machinery": [], "elsewhere": ["kernel/src/main.rs"]},
        ),
        (
            "lane C: net and netproto both match the bare `net` prefix",
            "C",
            ["net/tcp.rs", "netproto/src/lib.rs"],
            {"mine": ["net/tcp.rs", "netproto/src/lib.rs"], "machinery": [], "elsewhere": []},
        ),
        (
            "kernel/src/net is lane A's, NOT lane C's -- the prefix is anchored",
            "A",
            ["kernel/src/net/tftp.rs"],
            {"mine": ["kernel/src/net/tftp.rs"], "machinery": [], "elsewhere": []},
        ),
        (
            "and lane C must not claim it either",
            "C",
            ["kernel/src/net/tftp.rs"],
            {"mine": [], "machinery": [], "elsewhere": ["kernel/src/net/tftp.rs"]},
        ),
        (
            "a root document is nobody's scope and nobody's machinery",
            "A",
            ["known-issues.md"],
            {"mine": [], "machinery": [], "elsewhere": ["known-issues.md"]},
        ),
        (
            "windows separators are normalised before matching",
            "A",
            ["kernel\\src\\bench.rs"],
            {"mine": ["kernel/src/bench.rs"], "machinery": [], "elsewhere": []},
        ),
        (
            "an unknown lane claims nothing rather than everything",
            "Z",
            ["kernel/src/main.rs", "scripts/x.py"],
            {"mine": [], "machinery": ["scripts/x.py"], "elsewhere": ["kernel/src/main.rs"]},
        ),
    ]

    failures = 0
    for name, lane, paths, want in cases:
        got = classify(paths, lane)
        if got == want:
            print(f"  ok    {name}")
        else:
            failures += 1
            print(f"  FAIL  {name}")
            print(f"        want {want}")
            print(f"        got  {got}")

    print(f"merge-readiness: self-test {'passed' if not failures else 'FAILED'} "
          f"({failures} failure(s), {len(cases)} case(s))")
    return 1 if failures else 0


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--self-test", "--selftest", action="store_true",
                    help="run the classifier's own test cases and exit")
    args = ap.parse_args(argv)
    if args.self_test:
        return self_test()
    return report()


if __name__ == "__main__":
    sys.exit(main())
