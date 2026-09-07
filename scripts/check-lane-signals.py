#!/usr/bin/env python3
"""Cross-lane operational signalling, over the one directory all lanes share.

WHY THIS EXISTS
---------------
`requests/` is the channel for *technical* exchange between lanes and it is
good at that: durable, attributable, reviewable, and in git. It is useless for
*operational* messages -- "stop at your next clean point", "I am about to move
the tree", "who is running a boot test?" -- for one structural reason: a
request lives on a branch, so it is invisible to the addressee until they
`git fetch && git merge origin/main`. On 2026-09-06 lane A needed to tell B and
C to stop before a drive migration and had no way to do it; the operator had to
carry the message by hand.

The fix is not a faster version of `requests/`. It is a different medium for a
different traffic class:

    requests/  technical, durable, per-branch, read at task start
    here       operational, ephemeral, shared by ALL lanes, read every turn

WHY THE GIT COMMON DIR
----------------------
Every worktree shares one git common directory -- verified: all three lanes
report `D:/visual studio projects/os/.git`. A file there is visible to all
three *immediately*, on every branch, with no merge and no push. That is
exactly the property an operational signal needs, and the boot lock
(`$_common_git/slateos-boot-lock`) already relies on it, so the pattern is
proven rather than novel.

It is deliberately NOT in the worktree: an operational signal must not become a
commit, must not conflict, and must not survive into history.

WHAT A SIGNAL IS NOT
--------------------
Not a chat room, and not a mailbox that expects replies. Everything here is
one-way and imperative. The measured argument against a conversational channel
is that the last cross-lane bug to cost real time -- `test-canary-load`'s live
cases failing on a busy host -- was filed 2026-09-03, reached lane A's tree on
2026-09-04, and still cost lane A a 7783 s boot test on 2026-09-06. Transport
was never the problem; attention was. A channel that delivers faster would not
have helped, so this one is built to be *checked*, not to be talked on.
"""

from __future__ import annotations

import argparse
import datetime as _dt
import importlib.util
import os
import subprocess
import sys
from pathlib import Path

LANES = ("A", "B", "C")

#: Presence of this file means: every lane stops at its next clean point.
HALT = "HALT"

#: A notice is `notice-<to>-<from>-<stamp>.md`; `to` may be `all`.
NOTICE_PREFIX = "notice-"


def _detect_lane() -> str | None:
    """Reuse `which-lane.py`'s detector rather than re-deriving it.

    Imported by path because the module name has a hyphen. Duplicating the
    suffix table would give this file a second, quietly diverging opinion about
    which lane a session is -- and the table already carries a hard-won detail
    (one config directory has a trailing space in its name).
    """
    here = Path(__file__).resolve().parent / "which-lane.py"
    spec = importlib.util.spec_from_file_location("which_lane", here)
    if spec is None or spec.loader is None:
        return None
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    lane, _cfg = mod.detect_lane()
    return lane


def signal_dir(root: Path | None = None) -> Path:
    """`<git-common-dir>/coordination`, created on demand.

    Resolved through git rather than assembled from a path, so it is correct in
    every worktree and survives the tree being moved to another drive.
    """
    cmd = ["git", "rev-parse", "--git-common-dir"]
    cwd = str(root) if root else None
    out = subprocess.run(cmd, cwd=cwd, capture_output=True, text=True, check=False)
    if out.returncode != 0:
        raise RuntimeError("not a git repository: cannot locate the shared signal directory")
    common = Path(out.stdout.strip())
    if not common.is_absolute():
        common = (Path(cwd) if cwd else Path.cwd()) / common
    return common.resolve() / "coordination"


def _now() -> str:
    return _dt.datetime.now(_dt.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def raise_halt(d: Path, reason: str, by: str) -> Path:
    d.mkdir(parents=True, exist_ok=True)
    p = d / HALT
    p.write_bytes(
        (f"raised-by: lane {by}\nraised-at: {_now()}\nreason: {reason}\n").encode("utf-8")
    )
    return p


def clear_halt(d: Path) -> bool:
    p = d / HALT
    if p.exists():
        p.unlink()
        return True
    return False


def post_notice(d: Path, to: str, frm: str, text: str) -> Path:
    d.mkdir(parents=True, exist_ok=True)
    stamp = _dt.datetime.now(_dt.timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    p = d / f"{NOTICE_PREFIX}{to.lower()}-{frm.lower()}-{stamp}.md"
    p.write_bytes((f"from: lane {frm}\nto: {to}\nat: {_now()}\n\n{text}\n").encode("utf-8"))
    return p


def pending(d: Path, lane: str | None) -> tuple[str | None, list[Path]]:
    """Return `(halt_text_or_None, notices_addressed_to_this_lane)`."""
    if not d.is_dir():
        return None, []
    halt = None
    hp = d / HALT
    if hp.is_file():
        halt = hp.read_bytes().decode("utf-8", errors="replace").strip()
    notices = []
    for p in sorted(d.glob(f"{NOTICE_PREFIX}*")):
        parts = p.name[len(NOTICE_PREFIX):].split("-")
        if not parts:
            continue
        target = parts[0]
        # `all` reaches everyone; an unknown lane is shown rather than hidden,
        # because a misaddressed notice nobody sees is worse than a stray one.
        if target == "all" or lane is None or target == lane.lower():
            notices.append(p)
    return halt, notices


def _self_test() -> int:
    """Fixtures over a throwaway directory: one true positive, one true negative.

    Both directions are asserted for every rule, because a checker with only
    positives passes for one that reports everything, and a checker with only
    negatives passes for one that reports nothing. Either alone certifies
    something that discriminates nothing.
    """
    import tempfile

    failures: list[str] = []

    def check(label: str, got: object, want: object) -> None:
        if got != want:
            failures.append(f"{label}: got {got!r}, want {want!r}")
            print(f"  FAIL {label}: got {got!r}, want {want!r}")
        else:
            print(f"  ok   {label}")

    with tempfile.TemporaryDirectory() as tmp:
        d = Path(tmp) / "coordination"

        # A directory that does not exist yet is quiet, not an error: the
        # common case is no signals at all, and that must cost nothing.
        check("no directory means nothing pending", pending(d, "A"), (None, []))

        d.mkdir(parents=True)
        check("an empty directory means nothing pending", pending(d, "A"), (None, []))

        # HALT is visible to every lane, which is the whole point of it.
        raise_halt(d, "migrating to E:", "A")
        for lane in LANES:
            halt, _ = pending(d, lane)
            check(f"lane {lane} sees the halt", halt is not None, True)
        halt, _ = pending(d, "B")
        check("the halt names who raised it", "raised-by: lane A" in (halt or ""), True)
        check("the halt carries the reason", "migrating to E:" in (halt or ""), True)

        # ...and clearing it really clears it, for everyone.
        check("clearing reports that it did something", clear_halt(d), True)
        check("clearing again reports it did not", clear_halt(d), False)
        for lane in LANES:
            halt, _ = pending(d, lane)
            check(f"lane {lane} no longer sees a halt", halt, None)

        # Addressing: a notice to B is for B, and is NOT for A or C.
        post_notice(d, "b", "a", "stop when convenient")
        _, to_b = pending(d, "B")
        check("the addressee sees the notice", len(to_b), 1)
        _, to_a = pending(d, "A")
        check("a lane not addressed does not", len(to_a), 0)
        _, to_c = pending(d, "C")
        check("nor does a third lane", len(to_c), 0)

        # `all` reaches everyone.
        post_notice(d, "all", "a", "tree is moving")
        for lane in LANES:
            _, got = pending(d, lane)
            want = 2 if lane == "B" else 1
            check(f"lane {lane} sees the broadcast", len(got), want)

    print()
    if failures:
        print(f"{len(failures)} FAILURE(S)")
        for f in failures:
            print(f"  - {f}")
        return 1
    print("check-lane-signals: self-test passed")
    return 0


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(
        description="Read or raise operational signals shared by every lane."
    )
    ap.add_argument("--self-test", "--selftest", dest="selftest", action="store_true",
                    help="run this checker's own fixtures and exit")
    ap.add_argument("--raise-halt", metavar="REASON",
                    help="ask every lane to stop at its next clean point")
    ap.add_argument("--clear-halt", action="store_true", help="lift the halt")
    ap.add_argument("--notice", metavar="TEXT", help="leave a one-way message")
    ap.add_argument("--to", default="all", help="notice addressee: a, b, c, or all")
    ap.add_argument("--quiet", action="store_true",
                    help="print nothing when there is nothing pending")
    args = ap.parse_args(argv)

    if args.selftest:
        return _self_test()

    lane = _detect_lane()
    try:
        d = signal_dir()
    except RuntimeError as e:
        print(f"check-lane-signals: {e}", file=sys.stderr)
        return 2

    if args.raise_halt:
        p = raise_halt(d, args.raise_halt, lane or "?")
        print(f"halt raised for every lane: {p}")
        return 0
    if args.clear_halt:
        print("halt lifted" if clear_halt(d) else "no halt was set")
        return 0
    if args.notice:
        p = post_notice(d, args.to, lane or "?", args.notice)
        print(f"notice left for {args.to}: {p}")
        return 0

    halt, notices = pending(d, lane)
    if halt is None and not notices:
        if not args.quiet:
            print(f"check-lane-signals: nothing pending for lane {lane or '?'}")
        return 0

    for p in notices:
        print(f"--- notice: {p.name}")
        print(p.read_bytes().decode("utf-8", errors="replace").rstrip())
    if halt is not None:
        print()
        print("=== HALT: every lane is asked to stop at its next clean point ===")
        print(halt)
        print()
        print("Commit and push what you have, then stop. Do not start another")
        print("task or a boot test. Lift it with --clear-halt once the reason")
        print("has passed.")
        # Non-zero so a caller that only checks the exit status still refuses.
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
