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
Every worktree shares one git common directory -- verified: every lane's
worktree reports `E:/visual studio projects/os/.git`. (Re-verified 2026-09-21
after the D:-to-E: migration; it said `D:` until then, which was true when
written and quietly wrong afterwards.) A file there is visible to every lane
*immediately*, on every branch, with no merge and no push. That is
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

TWO MORE DUTIES: THE OPERATOR'S ANSWERS, AND THE INTEGRATION TREE
-----------------------------------------------------------------
Both are here because this is the one script every lane runs at the start of
a task and on every wakeup, and both failed for want of exactly that.

The operator answers `open-questions/` by writing `open-questions-answers*.txt`
at the root of the integration tree (`E:/visual studio projects/os`) -- an
untracked file, on no branch, that no merge can show anyone. This reports any
such file, or later version of one, whose content hash is not yet in
`operator-answers/LEDGER.md` on any lane's branch (`operator-answers/README.md`).
It only reports: it never fails the run.

The integration tree is where every session is launched -- so where it reads
`CLAUDE.md` -- and where the operator reads the project. Nothing kept it
current: on 2026-10-09 it was 3,389 commits behind `main`, still showing the
operator the old single `open-questions.md` with every answered question in it
and none of the 33 then waiting, and still giving every session the `CLAUDE.md`
of 2026-09-26. So a run also fast-forwards it to `origin/main` -- `merge
--ff-only` and nothing else, which git refuses, changing nothing, when it would
overwrite a local change; the refusal is reported for the operator, never
forced. That fast-forward is the one write any lane makes in the integration
tree. Gates and hooks pass `--no-sync`, and a run inside a git hook never
syncs whatever it is passed (`GIT_DIR` there would aim the merge at the
repository being pushed).
"""

from __future__ import annotations

import argparse
import datetime as _dt
import hashlib
import importlib.util
import os
import re
import subprocess
import sys
from pathlib import Path

#: Every lane letter.  Must equal `which-lane.py`'s LANES -- the self-test
#: asserts it, because a lane missing here is a lane the self-test never checks
#: can see a halt.  Kept as a literal rather than imported so that this file,
#: which `boot-test.sh` runs as its first gate, still works if which-lane.py
#: itself is what is broken.
LANES = ("A", "B", "C", "D", "E", "F")

#: Presence of this file means: every lane stops at its next clean point.
HALT = "HALT"

#: A notice is `notice-<to>-<from>-<stamp>.md`; `to` may be `all`.
NOTICE_PREFIX = "notice-"

#: Notices older than this are silently expired: still on disk, but no longer
#: shown by default.  The value is deliberately short — notices are operational
#: ("the tree moved", "hub restarting"), not archival, and a pile of stale ones
#: trains readers to skim the output, which is worse than no output at all.
#: See requests/c-a-notices-never-expire-*.md for why this was added.
NOTICE_MAX_AGE = _dt.timedelta(days=3)


def _which_lane():
    """`which-lane.py` as a module, or None if it cannot be loaded.

    Imported by path because the module name has a hyphen.
    """
    here = Path(__file__).resolve().parent / "which-lane.py"
    spec = importlib.util.spec_from_file_location("which_lane", here)
    if spec is None or spec.loader is None:
        return None
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


def _detect_lane() -> str | None:
    """Reuse `which-lane.py`'s detector rather than re-deriving it.

    Duplicating it would give this file a second, quietly diverging opinion
    about which lane a session is.  That detector no longer reads
    `CLAUDE_CONFIG_DIR` -- two lanes share each account now -- but the worktree
    this script runs from, `SLATEOS_LANE` and `ORCH2_AGENT_NAME`; see its
    docstring.  None when it cannot tell, which `pending` treats as "show me
    everything" rather than "nothing is for me".
    """
    mod = _which_lane()
    if mod is None:
        return None
    lane, _how = mod.detect_lane()
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


def post_notice(d: Path, to: str, frm: str | None, text: str) -> Path:
    """Leave a notice for lane `to` (or `all`) from lane `frm`.

    `frm` is None when the sender is not a lane -- the operator's integration
    session, or a shell outside every lane worktree.  The sender goes into the
    file NAME, so it has to be a legal path component everywhere: this used to
    be passed as "?", which Windows refuses in a file name, so precisely the
    sender least able to name itself could not leave a notice at all.
    """
    d.mkdir(parents=True, exist_ok=True)
    stamp = _dt.datetime.now(_dt.timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    known = bool(frm) and frm.isalnum()
    tag = frm.lower() if known else "unknown"
    who = f"lane {frm}" if known else "a session that is not a lane"
    p = d / f"{NOTICE_PREFIX}{to.lower()}-{tag}-{stamp}.md"
    p.write_bytes((f"from: {who}\nto: {to}\nat: {_now()}\n\n{text}\n").encode("utf-8"))
    return p


def _notice_stamp(p: Path) -> _dt.datetime | None:
    """Parse the ISO-8601 timestamp from a notice filename, or ``None``.

    Filenames follow ``notice-<to>-<from>-<YYYYmmddTHHMMSSZ>.md``.
    The timestamp is the last hyphen-delimited segment before ``.md``.
    """
    stem = p.stem  # e.g. "notice-all-a-20260907T160557Z"
    parts = stem.split("-")
    if len(parts) < 4:
        return None
    raw = parts[-1]  # "20260907T160557Z"
    try:
        return _dt.datetime.strptime(raw, "%Y%m%dT%H%M%SZ").replace(
            tzinfo=_dt.timezone.utc
        )
    except ValueError:
        return None


def _notice_is_fresh(
    p: Path,
    now: _dt.datetime | None = None,
    max_age: _dt.timedelta = NOTICE_MAX_AGE,
) -> bool:
    """True if *p* was posted within *max_age* of *now* (default: wall clock)."""
    stamp = _notice_stamp(p)
    if stamp is None:
        # Cannot determine age — show it rather than hide it.
        return True
    if now is None:
        now = _dt.datetime.now(_dt.timezone.utc)
    return (now - stamp) <= max_age


def pending(
    d: Path,
    lane: str | None,
    *,
    include_expired: bool = False,
) -> tuple[str | None, list[Path]]:
    """Return `(halt_text_or_None, notices_addressed_to_this_lane)`.

    Notices older than :data:`NOTICE_MAX_AGE` are omitted unless
    *include_expired* is True.  The files are not deleted — they age out
    of the output, not out of existence.
    """
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
            if include_expired or _notice_is_fresh(p):
                notices.append(p)
    return halt, notices


# ---------------------------------------------------------------------------
# The operator's answers, and the integration tree (see the module docstring)
# ---------------------------------------------------------------------------

#: The operator's answers files, at the root of the integration tree, with any
#: suffix: `open-questions-answers.txt` (2026-09-07), then
#: `open-questions-answers.2.txt` (2026-09-27). A check for the first name
#: alone is how the second would be missed.
ANSWERS_GLOB = "open-questions-answers*.txt"

#: Where a processed answers file is recorded (`operator-answers/README.md`).
ANSWERS_LEDGER = "operator-answers/LEDGER.md"

#: A recorded hash: `sha256`, up to four characters of punctuation (a space, a
#: colon, a backtick), then sixty-four lowercase hex digits.
_LEDGER_SHA256 = re.compile(r"sha256[^0-9a-f]{0,4}([0-9a-f]{64})")

#: An answer keyed by its question's id at the start of a line: `A-Q14:`, or
#: `Q46:` from before the ids carried a lane letter.
_ANSWER_ID = re.compile(r"^[ \t]*((?:[A-F]-)?Q[0-9]+)[ \t]*:", re.MULTILINE)

#: An answer keyed by its question's title as the operator copied it:
#: `<title> (lane C, 2026-08-24): <answer>`.
_ANSWER_TITLED = re.compile(
    r"^(?![ \t]*(?:[A-F]-)?Q[0-9]+[ \t]*:).*\(lane ([A-F]), [0-9]{4}-[0-9]{2}-[0-9]{2}\):",
    re.MULTILINE,
)

_GITENV = None


def _gitenv():
    """`scripts/gitenv.py` as a module, or None if it cannot be loaded."""
    global _GITENV
    if _GITENV is None:
        here = Path(__file__).resolve().parent / "gitenv.py"
        try:
            spec = importlib.util.spec_from_file_location("gitenv", here)
            if spec is None or spec.loader is None:
                return None
            mod = importlib.util.module_from_spec(spec)
            spec.loader.exec_module(mod)
            _GITENV = mod
        except Exception:  # noqa: BLE001 -- any failure means "use the fallback"
            return None
    return _GITENV


def _clean_git_env(environ: dict[str, str] | None = None) -> dict[str, str]:
    """`environ` without the variables that bind git to one repository.

    Git exports `GIT_DIR` and its relatives into every hook, and they outrank
    both `cwd` and `-C` -- `scripts/gitenv.py` records what that has cost. Its
    list is used when it loads; otherwise every `GIT_*` goes, which is more
    than needed and never less.
    """
    base = dict(os.environ if environ is None else environ)
    mod = _gitenv()
    if mod is not None:
        return mod.clean_env(base)
    return {k: v for k, v in base.items() if not k.startswith("GIT_")}


def _in_git_hook(environ: dict[str, str] | None = None) -> bool:
    """Whether git is running us -- a hook, `bisect run`, `rebase --exec`."""
    env = os.environ if environ is None else environ
    return "GIT_DIR" in env or "GIT_INDEX_FILE" in env


def _git(args: list[str], cwd: Path) -> subprocess.CompletedProcess[str]:
    """`git <args>` in `cwd`, and in the repository `cwd` names -- never one an
    inherited `GIT_DIR` names."""
    return subprocess.run(
        ["git", *args],
        cwd=str(cwd),
        env=_clean_git_env(),
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        check=False,
    )


def answers_key(data: bytes) -> str:
    """The hash `ANSWERS_LEDGER` records for an answers file's content.

    SHA-256 with CRLF read as LF and the blank lines at either end ignored:
    the operator's file is CRLF and its copy in `operator-answers/` is stored
    LF, and the two are the same answers.
    """
    return hashlib.sha256(data.replace(b"\r\n", b"\n").strip()).hexdigest()


def answered(text: str) -> dict[str, list[str]]:
    """What an answers file answers, by lane: `{"A": ["A-Q14", ...], ...}`.

    Each id once, in the order answered. An id without a lane letter (`Q46`,
    from the single-agent era) is filed under `"?"`; each answer keyed by its
    question's title is counted under its lane as `"(by title)"`.
    """
    by_lane: dict[str, list[str]] = {}
    for m in _ANSWER_ID.finditer(text):
        qid = m.group(1)
        lane = qid[0] if qid[1] == "-" else "?"
        ids = by_lane.setdefault(lane, [])
        if qid not in ids:
            ids.append(qid)
    for m in _ANSWER_TITLED.finditer(text):
        by_lane.setdefault(m.group(1), []).append("(by title)")
    return by_lane


def recorded_keys(texts: list[str]) -> set[str]:
    """Every answers-file hash the ledger texts record."""
    keys: set[str] = set()
    for t in texts:
        keys.update(_LEDGER_SHA256.findall(t))
    return keys


def ledger_texts(worktree: Path) -> list[str]:
    """`ANSWERS_LEDGER` as this worktree has it, and as `main` and every lane
    branch on `origin` have it.

    Every lane branch, not just `main`: the lane that processes a file records
    it on its own branch first, and from that moment it is recorded --
    reporting it to the other five until it reaches `main` would be noise that
    trains everyone to skim this output.
    """
    texts = []
    local = worktree / ANSWERS_LEDGER
    if local.is_file():
        texts.append(local.read_bytes().decode("utf-8", errors="replace"))
    refs = _git(["for-each-ref", "--format=%(refname:short)", "refs/remotes/origin/"], worktree)
    for ref in refs.stdout.split():
        if ref == "origin/main" or ref.startswith("origin/lane-"):
            shown = _git(["show", f"{ref}:{ANSWERS_LEDGER}"], worktree)
            if shown.returncode == 0:
                texts.append(shown.stdout)
    return texts


def integration_tree(worktree: Path) -> Path | None:
    """The repository's main worktree -- `E:/visual studio projects/os`, where
    every session is launched and the operator reads and writes -- or None.

    Found through `--git-common-dir` from any worktree of the repository, so
    it is right wherever this runs from and survives the tree moving drives.
    """
    r = _git(["rev-parse", "--git-common-dir"], worktree)
    if r.returncode != 0:
        return None
    common = Path(r.stdout.strip())
    if not common.is_absolute():
        common = worktree / common
    common = common.resolve()
    tree = common.parent
    if common.name != ".git" or not (tree / ".git").is_dir():
        return None
    return tree


def unrecorded_answers(tree: Path, keys: set[str]) -> list[tuple[Path, dict[str, list[str]]]]:
    """Each answers file at the root of `tree` whose content is in no ledger,
    with what it answers."""
    found = []
    for p in sorted(tree.glob(ANSWERS_GLOB)):
        try:
            data = p.read_bytes()
        except OSError:
            continue
        if answers_key(data) not in keys:
            found.append((p, answered(data.decode("utf-8", errors="replace"))))
    return found


def sync_integration_tree(
    tree: Path, environ: dict[str, str] | None = None
) -> tuple[str, str] | None:
    """Fast-forward the integration tree to `origin/main` when it is behind.

    Returns `(kind, line)`, kind `"done"` or `"blocked"`, or None when there is
    nothing to say: it is current, git is running us (a hook), or another
    process holds its index at this moment.

    Only ever `merge --ff-only`, which git refuses -- changing nothing -- when
    it would overwrite a local change, so the operator's own edits there are
    never touched; the refusal is reported, not forced. Nothing here stashes,
    resets or checks out: the tree is the operator's. A tree on another branch,
    or holding a commit `main` lacks, is left alone and reported the same way.
    """
    if _in_git_hook(environ):
        return None
    branch = _git(["symbolic-ref", "--quiet", "--short", "HEAD"], tree).stdout.strip()
    if branch != "main":
        return ("blocked",
                f"the integration tree {tree} is on {branch or 'a detached HEAD'}, "
                "not main: left alone -- tell the operator")
    head = _git(["rev-parse", "--verify", "--quiet", "HEAD"], tree).stdout.strip()
    main = _git(["rev-parse", "--verify", "--quiet", "origin/main"], tree).stdout.strip()
    if not head or not main or head == main:
        return None
    if _git(["merge-base", "--is-ancestor", head, main], tree).returncode != 0:
        return ("blocked",
                f"the integration tree {tree} has commits origin/main does not: "
                "left alone -- tell the operator")
    behind = _git(["rev-list", "--count", f"{head}..{main}"], tree).stdout.strip() or "?"
    r = _git(["merge", "--ff-only", "--quiet", main], tree)
    if r.returncode == 0:
        return ("done",
                f"the integration tree {tree} was {behind} commit(s) behind main: "
                f"fast-forwarded to {main[:9]}")
    err = r.stderr.strip()
    if "index.lock" in err:
        # Another lane is fast-forwarding it at this moment.
        return None
    detail = " ".join(line.strip() for line in err.splitlines()[:4])
    return ("blocked",
            f"the integration tree {tree} is {behind} commit(s) behind main, and "
            f"git would not fast-forward it ({detail}). The operator's own "
            "uncommitted changes there are in the way: tell the operator. Never "
            "stash, reset or check out there.")


def _print_unrecorded(found: list[tuple[Path, dict[str, list[str]]]], lane: str | None) -> None:
    print()
    print("=== The operator has answered open questions, and no lane has recorded it ===")
    for path, by_lane in found:
        try:
            when = _dt.datetime.fromtimestamp(path.stat().st_mtime).strftime("%Y-%m-%d %H:%M")
        except OSError:
            when = "?"
        print(f"{path} (last written {when})")
        for key in sorted(by_lane, key=lambda k: (k != (lane or ""), k)):
            if lane and key == lane:
                who = f"lane {key} (you)"
            elif key == "?":
                who = "no lane letter"
            else:
                who = f"lane {key}"
            print(f"  for {who}: {' '.join(by_lane[key])}")
        if not by_lane:
            print("  (no question ids found in it: read it)")
    print("Process the whole file now, every lane's answers in it -- CLAUDE.md,")
    print('"When the operator answers". Its section in operator-answers/LEDGER.md')
    print("is what ends this message, for every lane.")


def _sync_fixtures(check) -> None:
    """`sync_integration_tree` against throwaway repositories: behind and clean
    (fast-forwarded, then nothing to say); a local edit in the way (refused,
    the edit and HEAD kept); run by git (nothing done); on another branch, and
    holding a commit of its own (left alone). And `integration_tree` found from
    a linked worktree.

    Every git call goes through a cleaned environment, and the scratch
    repository is checked to be the one git writes before the first write --
    `scripts/gitenv.py` has the two post-mortems that require both.
    """
    import tempfile

    env = _clean_git_env()

    def git(cwd: Path, *a: str) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            ["git", "-c", "user.name=fixture", "-c", "user.email=fixture@invalid",
             "-c", "commit.gpgsign=false", "-c", "core.autocrlf=false", *a],
            cwd=str(cwd), env=env, capture_output=True, text=True,
            encoding="utf-8", errors="replace", check=False)

    def write(p: Path, s: str) -> None:
        p.write_bytes(s.encode("utf-8"))

    tmp = Path(tempfile.mkdtemp(prefix="lane-signals-sync-"))
    try:
        origin = tmp / "origin"
        origin.mkdir()
        git(origin, "init", "-q", "-b", "main")
        gd = git(origin, "rev-parse", "--absolute-git-dir").stdout.strip()
        if not gd or Path(gd).resolve() != (origin / ".git").resolve():
            check("the fixture repository is the scratch one", gd, str(origin / ".git"))
            return
        write(origin / "f.txt", "one\n")
        git(origin, "add", "f.txt")
        git(origin, "commit", "-q", "-m", "one")
        tree = tmp / "os"
        git(tmp, "clone", "-q", str(origin), str(tree))

        def head() -> str:
            return git(tree, "rev-parse", "HEAD").stdout.strip()

        # Behind, and clean: fast-forwarded.
        write(origin / "g.txt", "two\n")
        git(origin, "add", "g.txt")
        git(origin, "commit", "-q", "-m", "two")
        git(tree, "fetch", "-q")
        got = sync_integration_tree(tree, environ={})
        check("a tree behind main is fast-forwarded", got is not None and got[0], "done")
        check("onto main", head(), git(tree, "rev-parse", "origin/main").stdout.strip())
        check("and a current tree has nothing to say", sync_integration_tree(tree, environ={}), None)

        # A local edit main would overwrite: refused, everything kept.
        write(tree / "f.txt", "the operator's edit\n")
        write(origin / "f.txt", "three\n")
        git(origin, "commit", "-q", "-am", "three")
        git(tree, "fetch", "-q")
        before = head()
        got = sync_integration_tree(tree, environ={})
        check("an edit in the way is reported, naming the file",
              got is not None and got[0] == "blocked" and "f.txt" in got[1], True)
        check("and the edit is kept", (tree / "f.txt").read_bytes(), b"the operator's edit\n")
        check("and HEAD has not moved", head(), before)

        # Run by git, as a hook is: nothing at all.
        check("run from a hook, it does nothing",
              sync_integration_tree(tree, environ={"GIT_DIR": str(tree / ".git")}), None)
        git(tree, "checkout", "-q", "--", "f.txt")

        # On another branch: left alone.
        git(tree, "checkout", "-q", "-b", "elsewhere")
        got = sync_integration_tree(tree, environ={})
        check("a tree on another branch is left alone",
              (got is not None and got[0],
               git(tree, "rev-parse", "--abbrev-ref", "HEAD").stdout.strip()),
              ("blocked", "elsewhere"))
        git(tree, "checkout", "-q", "main")

        # Holding a commit main lacks: left alone.
        write(tree / "h.txt", "mine\n")
        git(tree, "add", "h.txt")
        git(tree, "commit", "-q", "-m", "mine")
        mine = head()
        got = sync_integration_tree(tree, environ={})
        check("a tree with a commit main lacks is left alone",
              (got is not None and got[0], head()), ("blocked", mine))

        # Found from a linked worktree, as every lane's is.
        git(tree, "worktree", "add", "-q", "-b", "lane", str(tmp / "lane"))
        check("a linked worktree finds the main one",
              integration_tree(tmp / "lane"), tree.resolve())
        check("and the main one finds itself", integration_tree(tree), tree.resolve())
    finally:
        mod = _gitenv()
        if mod is not None:
            mod.remove_tree(tmp)
        else:
            import shutil
            shutil.rmtree(tmp, ignore_errors=True)


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

        # Addressing: a notice to B is for B, and is NOT for any other lane.
        post_notice(d, "b", "a", "stop when convenient")
        _, to_b = pending(d, "B")
        check("the addressee sees the notice", len(to_b), 1)
        for lane in LANES:
            if lane == "B":
                continue
            _, got = pending(d, lane)
            check(f"lane {lane}, not addressed, does not", len(got), 0)

        # `all` reaches everyone.
        post_notice(d, "all", "a", "tree is moving")
        for lane in LANES:
            _, got = pending(d, lane)
            want = 2 if lane == "B" else 1
            check(f"lane {lane} sees the broadcast", len(got), want)

        # A sender that is not a lane still gets a notice through, under a
        # file name every host accepts -- "?" used to go into the name.
        anon = post_notice(d, "all", None, "the lanes changed")
        check("a non-lane sender can post", anon.is_file(), True)
        check("and is named in a portable way", "-unknown-" in anon.name, True)
        check("and says so in the notice",
              "not a lane" in anon.read_text(encoding="utf-8"), True)
        anon.unlink()

        # --- Notice expiry ---
        # A notice that just landed is fresh.
        fresh = list(d.glob(f"{NOTICE_PREFIX}*"))
        check("freshly-posted notices are fresh",
              all(_notice_is_fresh(p) for p in fresh), True)

        # A notice with a timestamp 4 days ago is stale (> NOTICE_MAX_AGE=3d).
        four_days_ago = _dt.datetime.now(_dt.timezone.utc) - _dt.timedelta(days=4)
        stale_stamp = four_days_ago.strftime("%Y%m%dT%H%M%SZ")
        stale_file = d / f"{NOTICE_PREFIX}all-a-{stale_stamp}.md"
        stale_file.write_bytes(b"from: lane A\nto: all\nat: old\n\nstale\n")
        check("a 4-day-old notice is stale",
              _notice_is_fresh(stale_file), False)
        # ...and pending() does not return it.
        _, got_a = pending(d, "A")
        check("pending() omits the stale notice",
              stale_file not in got_a, True)
        # ...but include_expired=True brings it back.
        _, got_a_all = pending(d, "A", include_expired=True)
        check("include_expired shows it",
              stale_file in got_a_all, True)

        # An unparseable timestamp is shown rather than hidden (fail-open).
        bad_file = d / f"{NOTICE_PREFIX}all-x-BADSTAMP.md"
        bad_file.write_bytes(b"from: ?\nto: all\nat: ?\n\nbad\n")
        check("unparseable stamp is treated as fresh",
              _notice_is_fresh(bad_file), True)

    # --- The operator's answers ---
    crlf = b"A-Q14: A\r\nC-Q20: B, but...\r\n"
    lf = b"\nA-Q14: A\nC-Q20: B, but...\n\n"
    check("an answers file hashes the same with CRLF or LF",
          answers_key(crlf), answers_key(lf))
    check("and differently once an answer is added",
          answers_key(lf + b"B-Q9: real Oils\n") != answers_key(lf), True)
    text = ("F-Q2: do vp9.\n"
            "Q46: C, and do something\n"
            "I think A-Q12 is related -- not an answer, it does not start the line\n"
            "An account with no password: let it in? (lane C, 2026-08-24): C\n"
            "C-Q24: To enable... and A-Q13: is mid-line again\n"
            "F-Q2: said twice\n")
    check("answers are read by lane, each id once, titled ones counted",
          answered(text),
          {"F": ["F-Q2"], "?": ["Q46"], "C": ["C-Q24", "(by title)"]})
    ledger = "## x\n- **Content:** sha256 `" + answers_key(lf) + "`\n"
    check("a ledger's hashes are read", recorded_keys([ledger]), {answers_key(lf)})
    check("and text with no hash records nothing",
          recorded_keys(["sha256 of nothing yet"]), set())
    with tempfile.TemporaryDirectory() as tmp:
        tree = Path(tmp)
        (tree / "open-questions-answers.txt").write_bytes(crlf)
        # The suffix a check for the bare name missed.
        (tree / "open-questions-answers.2.txt").write_bytes(b"B-Q9: real Oils\n")
        (tree / "answers.txt").write_bytes(b"A-Q1: A\n")
        found = unrecorded_answers(tree, {answers_key(lf)})
        check("a recorded file is quiet, a suffixed one is found, others ignored",
              [(p.name, by) for p, by in found],
              [("open-questions-answers.2.txt", {"B": ["B-Q9"]})])
        check("with both recorded, nothing is found",
              unrecorded_answers(tree, {answers_key(lf), answers_key(b"B-Q9: real Oils")}),
              [])

    # --- The integration tree ---
    _sync_fixtures(check)

    # The lane list here must be which-lane.py's, or a lane is silently left
    # out of every loop above.
    mod = _which_lane()
    check("LANES matches which-lane.py",
          tuple(mod.LANES) if mod is not None else None, LANES)

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
    ap.add_argument("--to", default="all",
                    help="notice addressee: a lane letter (a-f), or all")
    ap.add_argument("--quiet", action="store_true",
                    help="print nothing when there is nothing pending")
    ap.add_argument("--include-expired", action="store_true",
                    help="show notices older than the expiry window too")
    ap.add_argument("--no-sync", action="store_true",
                    help="do not fast-forward the integration tree (gates and "
                         "hooks pass this: a check judges, it does not act)")
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
        # Refused rather than posted: a notice addressed to a lane that does
        # not exist is shown to no session that knows its own lane, so it
        # would be delivered to nobody and still report success.
        valid = {x.lower() for x in LANES} | {"all"}
        if args.to.lower() not in valid:
            print(f"check-lane-signals: --to {args.to!r} is not a lane; use one "
                  f"of {', '.join(sorted(valid))}", file=sys.stderr)
            return 2
        p = post_notice(d, args.to, lane, args.notice)
        print(f"notice left for {args.to}: {p}")
        return 0

    halt, notices = pending(d, lane, include_expired=args.include_expired)

    # The integration tree: kept current, and searched for answers.
    worktree = Path(__file__).resolve().parents[1]
    tree = integration_tree(worktree)
    unrecorded: list[tuple[Path, dict[str, list[str]]]] = []
    if tree is not None:
        if not args.no_sync:
            synced = sync_integration_tree(tree)
            # A fast-forward is news only to someone reading; a refusal is
            # for the operator, so it is said even under --quiet.
            if synced is not None and (synced[0] == "blocked" or not args.quiet):
                print(f"--- {synced[1]}")
        unrecorded = unrecorded_answers(tree, recorded_keys(ledger_texts(worktree)))

    if halt is None and not notices and not unrecorded:
        if not args.quiet:
            print(f"check-lane-signals: nothing pending for lane {lane or '?'}")
        return 0

    for p in notices:
        print(f"--- notice: {p.name}")
        print(p.read_bytes().decode("utf-8", errors="replace").rstrip())
    if unrecorded:
        _print_unrecorded(unrecorded, lane)
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
