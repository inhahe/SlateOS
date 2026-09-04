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
* `57d21b4ee` (2026-08-25) --
  `c-b-sed-test-fixtures-share-one-path-across-processes.md`, still missing
  until 2026-08-29.
* `cd23f2f97` (lane C, 2026-08-29) --
  `a-c-scratch-target-dir-outliving-its-job.md`, and the reply that lane C
  filed the same day *cites it by name in its own first line*, so the deletion
  broke the reply's only pointer at the thing it was replying to.
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
must still exist in the tree being judged -- the working tree by default, or the
commit named by `--head`, which is what the push hook passes. That base is the
last commit the judged tree shares with the trunk, so the comparison sees
exactly what *this lane* removed since diverging and nothing another lane did --
which is what makes it usable in three worktrees at once without one lane's
history indicting another.

Both of this gate's inputs come from that one tree: the deletions and the
waivers that excuse them. See "The escape hatch" below for why the second half
matters as much as the first.

Deletions are compared with rename detection on, so moving a file (fixing a
slug, or sweeping an entry into an archive directory) is a rename and passes.
Only an actual disappearance fails.

Every path counts, not only `*.md`: a request that argues from a measurement or
a capture cites that file by path just as other documents cite the request, so
losing the attachment breaks a citation in exactly the same way. The two
exceptions are in `MACHINERY` below, and they are exceptions because the advice
this script gives is incoherent when applied to them, not because they matter
less.

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

**The waiver is read from the same tree as the deletion.** Under `--head` that
is the commit being pushed, not the disk -- because "deliberate and reviewable"
is a claim about something that is being published. A waiver read off the
working tree while the deletion is read from the commit can be written, used,
and dropped without ever being committed: add the basename, push the deletion,
`git checkout` the waiver away, and the request is gone from shared history with
no record that an override was ever claimed. That is a worse hole than the
staged restore `--head` was added for, since a staged restore at least leaves
the file present somewhere.

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
    python scripts/check-requests-not-deleted.py --head Y  # judge commit Y, not
                                                          # the working tree
    python scripts/check-requests-not-deleted.py --selftest

Exit status: 0 clean (or skipped, see below), 1 a request was deleted, 2 the
repository could not be read at all. A worktree with no `origin/main` and no
`main` -- a fresh clone that has fetched nothing -- is reported as SKIP and
exits 0, because that state means "no history to compare", not "no deletions".
"""

from __future__ import annotations

import argparse
import os
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

# `gitenv.clean_env()` is load-bearing on every subprocess below, not hygiene.
# An inherited `GIT_DIR` -- which git exports into every hook, and this script
# is run from `pre-push` -- outranks both `cwd=` and `-C`, so without it the
# self-test's fixture commands write to the repository being pushed. They once
# did; see `scripts/gitenv.py` for what that cost and `f0534726e` for the
# repair.
import gitenv  # noqa: E402

# Only `GitTree` is wanted here, not `open_tree`: the allowlist is one small
# file, and `RevTree` would build a `git ls-tree -r` index of the whole
# repository (13,821 paths, ~3.8 s -- see its docstring) to answer a question
# about one path. `GitTree.read` is the same seam without the index, and it
# already returns `None` for "not in that tree", which is the answer this gate
# gets almost every time it asks.
import gittree  # noqa: E402

ROOT = Path(__file__).resolve().parent.parent
REQUESTS = "requests/"

# Repo-relative, and deliberately not a `ROOT /` path: this file has to be read
# out of a *commit* as often as off the disk, and a path bound to ROOT at import
# time can only name the disk. It is also one global rather than two, which the
# self-test cares about -- it repoints ROOT, and a second global derived from
# the old one is a self-test that silently reads the real repository.
ALLOWLIST_REL = "requests/.deletions-allowed"

# Files under `requests/` that are this gate's own plumbing rather than anyone's
# argument. They are exempt because the advice this script gives is incoherent
# when applied to them: `.gitkeep` cannot carry a `**Status:**` line, and
# reporting the deletion of `.deletions-allowed` would tell the reader to fix it
# by adding a basename to the very file they just removed. Both are also safe to
# lose -- `.gitkeep` is meaningless once the directory has 186 real files in it,
# and dropping the allowlist withdraws every waiver, which fails toward strict.
MACHINERY = frozenset({".gitkeep", ".deletions-allowed"})

# Preference order for the trunk to compare against. `origin/main` is the real
# trunk; local `main` is the fallback for a worktree whose remote ref is missing
# but which still has the branch (the `os` integration checkout, for one).
TRUNK_CANDIDATES = ("origin/main", "main")


def _git(*args: str) -> tuple[int, str]:
    """Run git in the repo root, returning (returncode, stdout+stderr).

    `cwd=ROOT` is the whole of how this script picks a repository, which is why
    the environment is scrubbed: an inherited `GIT_DIR` would silently override
    it and every answer below would be about some other repository.
    """
    proc = subprocess.run(
        ["git", *args],
        cwd=str(ROOT),
        env=gitenv.clean_env(),
        capture_output=True,
        text=True,
        check=False,
    )
    return proc.returncode, (proc.stdout or "") + (proc.stderr or "")


def _rev_exists(rev: str) -> bool:
    rc, _ = _git("rev-parse", "--verify", "--quiet", f"{rev}^{{commit}}")
    return rc == 0


def _parse_allowlist(text: str) -> dict[str, str]:
    """Basenames mapped to the stated reason. Shared by both readers below.

    One parser for both provenances, so that a waiver cannot mean one thing on
    the disk and another in a commit. That is not hypothetical tidiness: the two
    readers were one function until this file learned to read a revision, and
    the whole defect that prompted the split was the two *inputs* disagreeing
    about which tree they came from.
    """
    allowed: dict[str, str] = {}
    for raw in text.splitlines():
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        name, _, reason = line.partition("#")
        name = name.strip()
        if name:
            allowed[name] = reason.strip() or "(no reason given)"
    return allowed


def load_allowlist(head: str | None = None) -> dict[str, str]:
    """Basenames that may be deleted, mapped to the stated reason.

    `head` selects the tree, and it must be the *same* tree `deleted_since` is
    given. The two are one question -- "does this tree delete a request it does
    not waive?" -- and answering its halves from different trees is what let a
    waiver be claimed without being published; see "The escape hatch" above.

    Missing file is not an error, on either side: the common case is that
    nothing is allowed, and requiring an empty file to exist would be one more
    thing to forget. `GitTree.read` already spells absence as `None`, and the
    disk arm is written to match it rather than the other way round.

    Note what a missing allowlist means here, since the sibling gates make the
    opposite choice: absent waives nothing, so it fails *toward strict*. That is
    the reverse of `quote-names`' baseline, where an absent file would forgive
    nothing and therefore accuse everything -- which is why that gate refuses to
    report a verdict without one, and this gate is content to carry on.
    """
    if head is None:
        path = ROOT / ALLOWLIST_REL.replace("/", os.sep)
        if not path.is_file():
            return {}
        return _parse_allowlist(path.read_text(encoding="utf-8", errors="replace"))
    with gittree.GitTree(str(ROOT)) as git_tree:
        raw = git_tree.read(head, ALLOWLIST_REL)
    if raw is None:
        return {}
    return _parse_allowlist(raw.decode("utf-8", errors="replace"))


def deleted_since(base: str, head: str | None = None) -> list[str]:
    """Paths under `requests/` present at `base` and absent from `head`.

    `head=None` means the working tree, which is what the build gates want: they
    ask "is a request missing right now", and they want the answer before the
    deletion is even committed.

    A push gate must ask a different question, and passing `head` is how. It
    judges *the commit being published*, not the worktree, and the difference
    matters in both directions:

    * **False negative.** Commit X deletes `requests/foo.md` and is about to be
      pushed; the file has since been restored and *staged*. Diffed against the
      worktree there is no deletion, so the gate passes and X enters shared
      history -- the exact event the gate exists to prevent, missed because of a
      change that is not being pushed. (Staged, not merely present: `git diff
      <base>` cannot see an untracked file, so an unstaged restore hides
      nothing. `git add` is the ordinary way to put a file back, and a merge
      that reintroduces one stages it for you.)
    * **False positive.** The mirror image: an uncommitted deletion blocks a
      push of unrelated clean commits.

    `-M` turns a move into an R and keeps it out of this list, so renaming a
    slug or sweeping into an archive directory passes; `--diff-filter=D` then
    leaves only real disappearances.
    """
    args = ["diff", "-M", "--diff-filter=D", "--name-only", base]
    if head is not None:
        args.append(head)
    rc, out = _git(*args, "--", REQUESTS)
    if rc != 0:
        raise RuntimeError(out.strip())
    return [ln.strip() for ln in out.splitlines() if ln.strip()]


def selftest() -> int:
    """Verify the detector against a throwaway repository with known history.

    Why a gate needs one: every failure mode of this script is silent and reads
    as good news. If `deleted_since` ever returns nothing -- a git option that
    changes meaning, a `-M` that starts matching too eagerly, a path filter that
    stops matching `requests/` -- the output is "clean", which is exactly what a
    healthy repository prints. Nothing distinguishes "no deletions" from "cannot
    see deletions", so the gate has to be asked a question it should fail.

    Several cases here are ways a *correct-looking* answer is wrong: a rename
    must not count (slug fixes and archive sweeps are routine, and a gate that
    blocked them would be bypassed within a week), the allowlist must still
    waive, and machinery must stay exempt.
    """
    import tempfile

    failures: list[str] = []

    def run(cwd: str, *args: str) -> str:
        # `env=gitenv.clean_env()` is load-bearing, not hygiene. Without it this
        # function writes to whatever repository the caller's environment names
        # -- see `scripts/gitenv.py`, and note that the caller is `pre-push`,
        # which always names one.
        proc = subprocess.run(
            ["git", *args], cwd=cwd, env=gitenv.clean_env(),
            capture_output=True, text=True, check=True,
        )
        return proc.stdout

    def expect(label: str, got: object, want: object) -> None:
        if got == want:
            print(f"  ok    {label}")
        else:
            print(f"  FAIL  {label}: got {got!r}, want {want!r}", file=sys.stderr)
            failures.append(label)

    global ROOT                                  # noqa: PLW0603 - see below
    saved_root = ROOT
    with tempfile.TemporaryDirectory(prefix="reqgate-") as tmp:
        run(tmp, "init", "--quiet", "-b", "main")
        run(tmp, "config", "user.email", "selftest@example.invalid")
        run(tmp, "config", "user.name", "selftest")
        # Rename detection is on by default in modern git, which means the
        # rename case below passes whether or not `deleted_since` still passes
        # `-M` -- the assertion would look load-bearing and not be. Turning it
        # off here makes the explicit `-M` the only thing that can save an
        # archive sweep, which is both what the test should prove and the
        # configuration this gate has to survive: `diff.renames=false` in a
        # lane's global config would otherwise turn every sweep into a refused
        # push.
        run(tmp, "config", "diff.renames", "false")
        reqs = os.path.join(tmp, "requests")
        os.makedirs(reqs)
        for name in ("a-b-one.md", "a-b-two.md", "a-b-swept.md", ".gitkeep"):
            with open(os.path.join(reqs, name), "w", encoding="utf-8", newline="") as fh:
                fh.write("# fixture\n")
        run(tmp, "add", "-A")
        run(tmp, "commit", "--quiet", "-m", "base")
        base = run(tmp, "rev-parse", "HEAD").strip()

        # `_git`, `deleted_since` and `load_allowlist` all resolve the repository
        # through `ROOT`. Repointing it is what lets the real code paths be
        # exercised rather than a reimplementation of them -- a self-test that
        # tested a copy of the logic would pass while the logic in use was
        # broken. One global does it now; it used to be two, and the second
        # (`ALLOWLIST`, bound to the old ROOT at import time) is exactly the
        # kind of leftover that makes a fixture read the real repository.
        ROOT = Path(tmp)

        os.remove(os.path.join(reqs, "a-b-one.md"))
        os.remove(os.path.join(reqs, ".gitkeep"))
        os.makedirs(os.path.join(reqs, "archive"))
        os.rename(os.path.join(reqs, "a-b-swept.md"),
                  os.path.join(reqs, "archive", "a-b-swept.md"))
        run(tmp, "add", "-A")
        run(tmp, "commit", "--quiet", "-m", "delete one, sweep another")
        head = run(tmp, "rev-parse", "HEAD").strip()

        gone = deleted_since(base, head)
        expect("a deleted request is detected", "requests/a-b-one.md" in gone, True)
        expect("a swept (renamed) request is not a deletion",
               any("swept" in p for p in gone), False)
        expect("an untouched request is not reported",
               any("a-b-two" in p for p in gone), False)
        expect("machinery is seen by the diff (main() is what exempts it)",
               "requests/.gitkeep" in gone, True)
        expect("machinery is classified as exempt",
               [p for p in gone if Path(p).name in MACHINERY],
               ["requests/.gitkeep"])

        # The restore-in-worktree case: the reason --head exists at all.
        #
        # It must be *staged* to hide anything. `git diff <base>` compares
        # against the index-plus-worktree view, in which a path absent from the
        # index is simply absent -- so an untracked restore leaves the deletion
        # visible. That narrows the false negative but does not remove it.
        one = os.path.join(reqs, "a-b-one.md")
        with open(one, "w", encoding="utf-8", newline="") as fh:
            fh.write("# restored, uncommitted\n")
        expect("an untracked restore does not hide the deletion",
               "requests/a-b-one.md" in deleted_since(base), True)
        run(tmp, "add", "requests/a-b-one.md")
        expect("a STAGED restore hides the deletion from a worktree diff",
               "requests/a-b-one.md" in deleted_since(base), False)
        expect("...but not from --head, which judges the commit being pushed",
               "requests/a-b-one.md" in deleted_since(base, head), True)

        allowlist = Path(tmp) / "requests" / ".deletions-allowed"
        with open(allowlist, "w", encoding="utf-8", newline="") as fh:
            fh.write("# a comment\na-b-one.md  # folded into a-b-two.md\n")
        allowed = load_allowlist()
        expect("the allowlist waives by basename", "a-b-one.md" in allowed, True)
        expect("the allowlist keeps the stated reason",
               allowed.get("a-b-one.md"), "folded into a-b-two.md")
        expect("a comment line is not a waiver",
               any(k.startswith("#") for k in allowed), False)

        # Provenance. The allowlist above exists only on the disk, and `head`
        # is the commit that deleted the request -- so under `--head` the waiver
        # must not be visible, or a waiver can be claimed without ever being
        # published (see "The escape hatch" in the module docstring).
        #
        # This is the shape the sibling gates were converted for and this one
        # was not: `deleted_since` took `head` on 2026-09-02 and `load_allowlist`
        # did not, so for two days the gate read its deletions from the commit
        # and its permissions from the working tree.
        expect("an uncommitted waiver is invisible to --head",
               "a-b-one.md" in load_allowlist(head), False)
        run(tmp, "add", "-A")
        run(tmp, "commit", "--quiet", "-m", "record the waiver")
        waived_head = run(tmp, "rev-parse", "HEAD").strip()
        # The control. Without it the assertion above is satisfied by a reader
        # that returns `{}` for every revision, which is a different bug with
        # the same green -- and one that would refuse every legitimate sweep.
        expect("...and a committed one is not",
               load_allowlist(waived_head).get("a-b-one.md"),
               "folded into a-b-two.md")
        # The earlier commit still does not carry it. `git cat-file` answering
        # per-revision rather than "the newest version it can find" is the whole
        # mechanism, and it is worth one assertion.
        expect("a waiver is not backdated onto the commit before it",
               "a-b-one.md" in load_allowlist(head), False)
        expect("a tree with no allowlist at all waives nothing",
               load_allowlist(base), {})

    ROOT = saved_root

    if failures:
        print(f"\ncheck-requests-not-deleted: SELF-TEST FAILED "
              f"({len(failures)}): {', '.join(failures)}", file=sys.stderr)
        return 1
    print("check-requests-not-deleted: self-test passed")
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument(
        "--base",
        default=None,
        help="commit to compare against (default: merge-base with origin/main)",
    )
    ap.add_argument(
        "--head",
        default=None,
        help="commit to judge (default: the working tree). The push hook passes "
             "the commit being published, so a staged restore cannot hide a "
             "deletion that is about to become shared history.",
    )
    ap.add_argument(
        "--selftest",
        action="store_true",
        help="build a throwaway repository and verify this gate still detects a "
             "deletion, ignores a rename, and honours the allowlist",
    )
    args = ap.parse_args()

    if args.selftest:
        return selftest()

    rc, _ = _git("rev-parse", "--git-dir")
    if rc != 0:
        print(
            "check-requests-not-deleted: not a git repository; cannot compare",
            file=sys.stderr,
        )
        return 2

    if args.head is not None and not _rev_exists(args.head):
        print(
            f"check-requests-not-deleted: --head {args.head!r} is not a commit",
            file=sys.stderr,
        )
        return 2
    # The merge base is taken against whatever is being judged. Using HEAD's
    # merge base while diffing some other commit would compare two unrelated
    # points and report every request that differs between them.
    tip = args.head or "HEAD"

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
        rc, out = _git("merge-base", tip, trunk)
        if rc != 0:
            # Unrelated histories, or a HEAD with no commits. Either way there
            # is nothing to diff, and that is not a violation.
            print(
                f"check-requests-not-deleted: SKIP -- no merge base between "
                f"{tip} and {trunk}."
            )
            return 0
        base = out.strip().splitlines()[0]

    try:
        gone = deleted_since(base, args.head)
    except RuntimeError as exc:
        print(f"check-requests-not-deleted: git diff failed: {exc}", file=sys.stderr)
        return 2

    try:
        allowed = load_allowlist(args.head)
    except gittree.GitTreeError as exc:
        # `--head` was verified to be a commit above, so this is git itself
        # failing to start, not a bad revision. Exit 2, because run-checker.sh
        # reads 1 as "this gate found a deleted request" and would print the
        # whole restore-it advice over a failure that says nothing about
        # anybody's requests.
        print(f"check-requests-not-deleted: cannot read {ALLOWLIST_REL} at "
              f"{args.head}: {exc}", file=sys.stderr)
        return 2

    machinery = [p for p in gone if Path(p).name in MACHINERY]
    rest = [p for p in gone if Path(p).name not in MACHINERY]
    waived = [p for p in rest if Path(p).name in allowed]
    violations = [p for p in rest if Path(p).name not in allowed]

    for path in machinery:
        print(f"  note  {path} deleted; this gate's own plumbing, not a "
              f"request -- ignored")
    for path in waived:
        print(f"  note  {path} deleted, allowed by "
              f"requests/.deletions-allowed: {allowed[Path(path).name]}")

    if violations:
        for path in violations:
            print(f"  ERROR {path} was deleted", file=sys.stderr)

        # An attachment -- a measurement, a capture, the log a request argues
        # from -- is cited by path exactly as the request is, so it is restored
        # the same way. It just takes no status line of its own, and telling
        # someone to stamp a `.csv` would read as a bug in this script.
        attachments = [p for p in violations if not p.endswith(".md")]
        stamp_note = (
            "    # then add, e.g.:  **Status:** LANDED <date> by lane <x>\n"
            if len(attachments) < len(violations)
            else ""
        )
        attach_note = (
            "\n"
            f"  {len(attachments)} of these {'is' if len(attachments) == 1 else 'are'}"
            " not a request but a file one cites -- a\n"
            "  measurement, a capture, a log. Restore it the same way; it takes\n"
            "  no status line of its own.\n"
            if attachments
            else ""
        )

        print(
            "\ncheck-requests-not-deleted: FAILED "
            f"({len(violations)} deleted file"
            f"{'s' if len(violations) != 1 else ''} under requests/)\n"
            "\n"
            "  A landed request is stamped, not deleted (roadmap.md rule 2,\n"
            "  design-decisions.md 315). The file is the argument, and code and\n"
            "  documents across the tree cite it by path.\n"
            "\n"
            "  To fix, restore it:\n"
            f"    git checkout {base[:12]} -- " + " ".join(violations) + "\n"
            + stamp_note
            + attach_note
            + "\n"
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
