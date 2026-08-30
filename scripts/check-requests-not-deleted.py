#!/usr/bin/env python3
"""Gate: a `requests/` file may be stamped, but not deleted.

Why this exists
---------------

`roadmap.md` rule 2 used to end "Delete the file when it lands." It now says to
add a ``**Status:** ...`` line and leave the file where it is, for the reason in
`design-decisions.md` §315: a request is not a ticket, it is the *argument* --
the measurement, the ten-row table, the reasoning that settled a design -- and
about twenty things across the tree cite one by path. Deleting it turns every
one of those citations into a dead end, and, worse, into an unanswerable
question: a reader who follows a missing path cannot tell whether the request
was answered, withdrawn, or never existed.

The convention was enforced by attention, and attention lost four times. Rule 2
changed in `236dc2206`, 2026-08-16 09:47; every commit below is after it:

* `d30e2a5ca` (lane A, 2026-08-16 11:35) -- one hour and 48 minutes later.
* `57d21b4ee` (2026-08-25) -- `c-b-sed-test-fixtures-share-one-path-across-
  processes.md`, still missing until 2026-08-29.
* `cd23f2f97` (lane C, 2026-08-29) -- `a-c-scratch-target-dir-outliving-its-
  job.md`, and the reply that lane C filed the same day *cites it by name in
  its own first line*, so the deletion broke the reply's only pointer at the
  thing it was replying to.
* `dd4e34fd9` (lane A, 2026-08-29) -- two more, and its own commit message
  asserted the *opposite* rule. That is the telling one: the author was not
  ignoring the convention but misremembering it, which no reminder fixes. The
  symptom arrived within three minutes -- the next commit had to repoint two
  live citations at something that still existed, exactly the failure §315
  describes.

So this is not one lane being careless. It is every lane, spread over two
weeks, in commits whose messages are otherwise careful.

`scripts/open-requests.py` cannot help. It answers "which surviving files are
unresolved?", and a deleted file survives nothing, so a deletion makes a request
vanish from the one report that exists to find it -- silently, and in the
direction that reads as "nothing is open". Only a diff against history can see
a deletion at all.

What it checks
--------------

Every path under `requests/` that exists at the merge base with `origin/main`
must still exist in the working tree. That base is the last commit this branch
shares with the trunk, so the comparison sees exactly what *this lane* removed
since diverging and nothing another lane did -- which is what makes it usable in
three worktrees at once without one lane's history indicting another.

Deletions are compared with rename detection on, so moving a file (fixing a
slug, or sweeping an entry into an archive directory) is a rename and passes.
Only an actual disappearance fails.

Uncommitted deletions count. `git diff <base>` compares the base against the
*working tree*, so a `rm` that has not been committed yet is caught before it
becomes history rather than after -- which is the whole point, since the cost of
this mistake is paid by whoever reads the citation months later.

The escape hatch
----------------

`requests/.deletions-allowed` lists basenames that may legitimately go, one per
line, each with a reason after a `#`. It exists because "never delete" is a
strong claim about a directory that has already had one archive sweep, and a
gate with no override gets disabled rather than obeyed. Adding a line to it is a
deliberate, reviewable act, which is all this gate is really asking for.

What it cannot see
------------------

A deletion that has already been merged to `main` moves the base past itself and
becomes invisible here. This gate is therefore a pre-merge check, not an audit
of history -- it catches the mistake in the window where it is free to fix, and
both incidents above were caught (late) by a human reading, not by a tool. If a
past deletion needs finding, `git log --diff-filter=D -- requests/` is the query.

Usage
-----

    python scripts/check-requests-not-deleted.py           # gate; 0 ok, 1 fail
    python scripts/check-requests-not-deleted.py --base X  # compare against X

Exit status: 0 clean (or skipped, see below), 1 a request was deleted, 2 the
repository could not be read at all. A worktree with no `origin/main` and no
`main` -- a fresh clone that has fetched nothing -- is reported as SKIP and
exits 0, because that state means "no history to compare", not "no deletions".
"""

from __future__ import annotations

import argparse
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
REQUESTS = "requests/"
ALLOWLIST = ROOT / "requests" / ".deletions-allowed"

# Preference order for the trunk to compare against. `origin/main` is the real
# trunk; local `main` is the fallback for a worktree whose remote ref is missing
# but which still has the branch (the `os` integration checkout, for one).
TRUNK_CANDIDATES = ("origin/main", "main")


def _git(*args: str) -> tuple[int, str]:
    """Run git in the repo root, returning (returncode, stdout+stderr)."""
    proc = subprocess.run(
        ["git", *args],
        cwd=str(ROOT),
        capture_output=True,
        text=True,
        check=False,
    )
    return proc.returncode, (proc.stdout or "") + (proc.stderr or "")


def _rev_exists(rev: str) -> bool:
    rc, _ = _git("rev-parse", "--verify", "--quiet", f"{rev}^{{commit}}")
    return rc == 0


def load_allowlist() -> dict[str, str]:
    """Basenames that may be deleted, mapped to the stated reason.

    Missing file is not an error: the common case is that nothing is allowed,
    and requiring an empty file to exist would be one more thing to forget.
    """
    allowed: dict[str, str] = {}
    if not ALLOWLIST.is_file():
        return allowed
    for raw in ALLOWLIST.read_text(encoding="utf-8", errors="replace").splitlines():
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        name, _, reason = line.partition("#")
        name = name.strip()
        if name:
            allowed[name] = reason.strip() or "(no reason given)"
    return allowed


def deleted_since(base: str) -> list[str]:
    """Paths under `requests/` present at `base` and absent from the worktree.

    `-M` turns a move into an R and keeps it out of this list; `--diff-filter=D`
    then leaves only real disappearances.
    """
    rc, out = _git(
        "diff", "-M", "--diff-filter=D", "--name-only", base, "--", REQUESTS
    )
    if rc != 0:
        raise RuntimeError(out.strip())
    return [ln.strip() for ln in out.splitlines() if ln.strip()]


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument(
        "--base",
        default=None,
        help="commit to compare against (default: merge-base with origin/main)",
    )
    args = ap.parse_args()

    rc, _ = _git("rev-parse", "--git-dir")
    if rc != 0:
        print(
            "check-requests-not-deleted: not a git repository; cannot compare",
            file=sys.stderr,
        )
        return 2

    if args.base:
        base = args.base
        if not _rev_exists(base):
            print(
                f"check-requests-not-deleted: --base {base!r} is not a commit",
                file=sys.stderr,
            )
            return 2
    else:
        trunk = next((t for t in TRUNK_CANDIDATES if _rev_exists(t)), None)
        if trunk is None:
            print(
                "check-requests-not-deleted: SKIP -- no "
                + " or ".join(TRUNK_CANDIDATES)
                + " in this worktree, so there is no trunk to compare against."
            )
            return 0
        rc, out = _git("merge-base", "HEAD", trunk)
        if rc != 0:
            # Unrelated histories, or a HEAD with no commits. Either way there
            # is nothing to diff, and that is not a violation.
            print(
                f"check-requests-not-deleted: SKIP -- no merge base between "
                f"HEAD and {trunk}."
            )
            return 0
        base = out.strip().splitlines()[0]

    try:
        gone = deleted_since(base)
    except RuntimeError as exc:
        print(f"check-requests-not-deleted: git diff failed: {exc}", file=sys.stderr)
        return 2

    allowed = load_allowlist()
    violations = [p for p in gone if Path(p).name not in allowed]
    waived = [p for p in gone if Path(p).name in allowed]

    for path in waived:
        print(f"  note  {path} deleted, allowed by "
              f"requests/.deletions-allowed: {allowed[Path(path).name]}")

    if violations:
        for path in violations:
            print(f"  ERROR {path} was deleted", file=sys.stderr)
        print(
            "\ncheck-requests-not-deleted: FAILED "
            f"({len(violations)} deleted request"
            f"{'s' if len(violations) != 1 else ''})\n"
            "\n"
            "  A landed request is stamped, not deleted (roadmap.md rule 2,\n"
            "  design-decisions.md 315). The file is the argument, and code and\n"
            "  documents across the tree cite it by path.\n"
            "\n"
            "  To fix, restore it and add a status line under the title:\n"
            f"    git checkout {base[:12]} -- " + " ".join(violations) + "\n"
            "    # then add, e.g.:  **Status:** LANDED <date> by lane <x>\n"
            "\n"
            "  Use an open/blocked/partial wording instead if only part of it\n"
            "  landed -- scripts/open-requests.py ranks that above 'landed', so\n"
            "  an honest header is what keeps the unfinished half visible.\n"
            "\n"
            "  If a deletion really is right, add the basename and a reason to\n"
            "  requests/.deletions-allowed.",
            file=sys.stderr,
        )
        return 1

    print(
        f"check-requests-not-deleted: OK (base {base[:12]}, "
        f"{len(waived)} allowed deletion{'s' if len(waived) != 1 else ''})"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
