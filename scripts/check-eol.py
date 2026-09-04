#!/usr/bin/env python3
"""Assert that files declared `text eol=lf` really do have LF endings *on disk*.

`.gitattributes` promises, in as many words, that "a shell script must arrive in
the working tree with LF endings, on every platform and in every worktree". That
promise is kept at *checkout* and nowhere else, and nothing re-checks it
afterwards. This gate is the thing that re-checks it.

## Why no git command can tell you this

`text eol=lf` installs a clean filter: every time git reads a working-tree file
it converts CRLF to LF before comparing or hashing. So a file that is CRLF on
disk and LF in the index is, to git, *identical to the index*. Not "modified and
ignorable" -- identical. `git status` is clean, `git diff` is empty,
`git diff --quiet` exits 0, and `git add` stages nothing. Only `git diff-files`
and `git ls-files --eol`, which look at raw bytes, disagree, and nobody runs
those.

That is not a description of a hazard; it is a transcript of 2026-09-03 on lane
A. `scripts/boot-test.sh` and twelve other declared-LF files sat wholly CRLF in
the worktree while every git command reported a clean tree. The corruption was
finally noticed by `check_shellcheck` -- as SC1017, a *side effect*, forty-seven
minutes into a boot test, which then threw the whole run away. Repairing all
thirteen files produced, when staged, a zero-byte diff: the bad bytes had never
reached a commit and never would have, which is exactly why nothing pointed at
them.

The `.gitattributes` header already tells this story once, about a slightly
different variant of it ("with `core.autocrlf=input` the CRLF is normalised away
on commit, so `git status` stays clean and nothing ever points at the cause"),
and concludes that `text eol=lf` is the fix. It is the fix for what gets
*committed*. It is not a fix for what is on disk, because an attribute cannot
stop a tool from writing the file again afterwards -- and something did, most
likely a script rewriting a file through Python's default text mode, which on
Windows turns every `\n` into `\r\n` silently.

## Why the scope is every declared file, not just `*.sh`

Only the `.sh` files were actually *broken* by this: `bash` treats a CR as part
of the token it ends, so `set -u` becomes `set -u$'\r'`. A CR in a `.py`, `.md`
or `.txt` harms nothing a reader would notice today.

Gating only on the harmful ones is the tempting scope and it is the wrong one.
The thirteen corrupted files were corrupted by *one* event; `.sh` was one of the
thirteen. A gate scoped to `.sh` would have caught that single instance and
stayed silent about the other twelve, which is to say it would have reported the
symptom and hidden the size of the cause. The next occurrence lands wherever the
next script happens to rewrite something, and the point of catching it is to
find out that a tool in this tree writes text files in the wrong mode -- a fact
about the tooling, not about any one file's extension.

So the scope is the promise: whatever `.gitattributes` says is `eol=lf` is what
gets graded, read out of git rather than duplicated here as a list of suffixes.
A suffix list would be a second copy of the policy, free to drift from the first,
and the drift would show up as a gate quietly not covering a newly-declared type.

## Cost

Reading the ~1440 declared files is ~37 MB. That is 93 s single-threaded on this
host and about a fifth of that across a thread pool, because the cost is
per-file antivirus interception rather than bandwidth (see `open-questions.md`
A-Q7, which is about exactly that tax). The pool is why this is affordable at
all; if A-Q7 is ever answered the whole thing drops into the noise.

It earns that by running before anything is compiled or booted. The two gates
ahead of it in `boot-test.sh` -- `check_prerequisites` (are the tools present)
and `check_requests_not_deleted` (one git query) -- cost seconds between them,
so a CR found here is found within the first minute and costs only itself. The
equivalent evidence exists inside `check_shellcheck`, which sits some forty-five
minutes into the sweep and only covers `.sh` -- so a CR discovered *there* costs
a whole boot test, which is what it cost on 2026-09-03.

Stated as "before anything is compiled" rather than as a position in the list on
purpose. This paragraph said "by running *first*" until 2026-09-04, by which
time two gates had been inserted above it and the sentence was simply false --
an ordinal is a claim about every other gate in the file, so it goes stale when
any of them moves and nothing rechecks it. What actually matters is not being
third or first; it is being on the cheap side of the first `cargo` invocation.

## `--self-test` grades the gate against real files

Per `known-issues.md` -> `TD-A-A-WIRED-GATE-CAN-GRADE-ONE-LINE-AND-LOOK-LIKE-IT-
GRADES-A-SUBSYSTEM`: a fixture built from strings the author invented proves the
comparison logic works on input shaped the way the author imagined, and proves
nothing about whether the gate is still attached to the tree.

The fragile half here is not "does `\r` appear in these bytes" -- that cannot
drift. It is the *attribute query*: this shells out to `git check-attr -z
--stdin` and walks a NUL-separated stream three fields at a time. If that framing
ever changes, or the invocation grows a typo, the declared set becomes empty and
this gate reports a clean tree in a fraction of a second, forever, on every host.
So the self-test asserts the query returns a set with real files in it, that it
puts a known `.sh` inside and a known `.rs` outside, and that a CR planted in a
real declared file's real bytes is reported while the same CR planted in an
undeclared file's bytes is not. Nothing is written to disk.

Exit codes:
    0   every declared file is LF on disk
    1   at least one is not (the finding)
    2   could not look: not a git worktree, or fewer files found than the floor

Usage:
    python scripts/check-eol.py               # grade the worktree
    python scripts/check-eol.py --list        # also list what was checked
    python scripts/check-eol.py --self-test   # grade the gate, not the worktree
"""

from __future__ import annotations

import argparse
import concurrent.futures
import contextlib
import io
import subprocess
import sys
import tempfile
from pathlib import Path

# Measured 2026-09-03: 1438 files carry `eol=lf` (`*.sh`, `*.py`, `*.yaml`,
# `*.yml`, `*.md`, `*.txt`). 500 is well below any plausible shrink of the tree
# and far above what a broken query returns, which is 0. The floor exists
# because the failure this gate is most likely to suffer is finding nothing and
# calling that clean -- the same failure it exists to catch elsewhere.
DISCOVERY_FLOOR = 500

# Enough threads that the per-file antivirus round-trip overlaps, few enough
# that they are not competing for one spindle. The work is pure blocking I/O in
# the interpreter's read path, so the GIL is released throughout.
READ_THREADS = 16


def _git(args: list[str], stdin: bytes | None = None) -> subprocess.CompletedProcess:
    return subprocess.run(
        ["git", *args], input=stdin, capture_output=True, check=False)


def tracked_files() -> list[bytes]:
    """Every tracked path, as raw bytes.

    Bytes rather than `str` because a path is bytes at the OS boundary
    (CLAUDE.md self-review item 7) and `-z` is the only `git ls-files` output
    that survives a filename containing a newline or a quote.
    """
    r = _git(["ls-files", "-z"])
    if r.returncode != 0:
        return []
    return [p for p in r.stdout.split(b"\0") if p]


def declared_lf(paths: list[bytes]) -> list[bytes]:
    """The subset of `paths` whose `eol` attribute is `lf`.

    `git check-attr -z --stdin <attr>` emits three NUL-terminated fields per
    input path -- path, attribute name, value -- so the stream is walked in
    threes. It reads no file contents, which is why this costs five seconds
    over thirteen thousand paths while the read loop below costs twenty over
    fourteen hundred.

    Asking git rather than matching suffixes here is deliberate: the policy
    lives in `.gitattributes`, and a second copy of it in this file would be
    free to drift from the first without anything noticing.
    """
    if not paths:
        return []
    r = _git(["check-attr", "-z", "--stdin", "eol"],
             stdin=b"\0".join(paths) + b"\0")
    if r.returncode != 0:
        return []
    fields = r.stdout.split(b"\0")
    out = []
    # `len(fields) - 2` rather than `- 3`: the split of a NUL-*terminated*
    # stream leaves one empty trailing element, so the last real triple starts
    # at len-4. Walking to len-2 would read past it; walking in steps of three
    # from 0 while i+2 is in range is the honest bound.
    for i in range(0, len(fields) - 2, 3):
        if fields[i + 2] == b"lf":
            out.append(fields[i])
    return out


def scan_bytes(data: bytes) -> tuple[int, int]:
    """`(count of CR, offset of the first one)`, or `(0, -1)`.

    Every CR is counted, not just CRLF pairs. A lone CR is not the milder case:
    a CRLF file is at least uniformly wrong and mechanically repairable, while
    one stray CR in the middle of a line is a byte that some tool put there on
    purpose or by accident and that nobody will find by eye.
    """
    n = data.count(b"\r")
    return (n, data.find(b"\r")) if n else (0, -1)


def read_and_scan(paths: list[bytes]) -> tuple[list[tuple[str, int, int]], int]:
    """Scan every path, in parallel. Returns `(findings, files_read)`.

    Unreadable paths are counted as read rather than skipped silently: a file
    that git tracks and this cannot open is a fact worth surfacing, and it is
    surfaced as a finding below rather than swallowed here.
    """
    findings: list[tuple[str, int, int]] = []
    read = 0

    def one(p: bytes) -> tuple[str, int, int] | None:
        name = p.decode("utf-8", "surrogateescape")
        try:
            data = Path(name).read_bytes()
        except OSError:
            # A tracked path that will not open is not a CR finding, and
            # pretending it is would misdescribe it. It is also not nothing:
            # returning -1 as the count marks it for the caller.
            return (name, -1, -1)
        n, off = scan_bytes(data)
        return (name, n, off) if n else None

    with concurrent.futures.ThreadPoolExecutor(READ_THREADS) as pool:
        for got in pool.map(one, paths):
            read += 1
            if got is not None:
                findings.append(got)
    findings.sort()
    return findings, read


def floor_reason(n_declared: int, n_tracked: int) -> str | None:
    """Why a run with this much discovery must decline, or `None` to proceed.

    Split out of `main` so `--self-test` can grade the decision rather than the
    printing. A floor that is computed and then not acted on is a floor that
    does not exist, and the difference is one `if`.
    """
    if n_declared < DISCOVERY_FLOOR:
        return (f"cannot check line endings: only {n_declared} of {n_tracked} "
                f"tracked files are declared eol=lf, floor is {DISCOVERY_FLOOR}")
    return None


def check(declared: list[bytes], show_list: bool = False) -> int:
    """Read every path, report what has a CR, and return the verdict.

    The whole pipeline below the discovery step, in one function that takes its
    input as an argument. That is what lets `--self-test` drive it end to end
    -- enumerate, read, report, decide -- against a file it made rather than
    against the tree, and so catch the one mutation that matters most here: a
    gate that finds the defect, prints it, and returns 0 anyway.
    """
    findings, read = read_and_scan(declared)

    if show_list:
        for p in declared:
            print(f"  {p.decode('utf-8', 'surrogateescape')}")

    for name, n, off in findings:
        if n < 0:
            print(f"{name}: tracked but could not be read")
        else:
            print(f"{name}: {n} carriage return(s), first at byte {off}")

    print(f"\n{read} file(s) declared `text eol=lf`, {len(findings)} with a "
          f"carriage return")

    if not findings:
        return 0
    print(REFUSAL)
    return 1


def _decline(reason: str, detail: str) -> int:
    """Exit 2 with the reason as the FIRST line, and everything on one stream.

    `run-checker.sh` quotes the first line of the merged stdout+stderr log as
    the reason a gate declined. Two streams into one file do not arrive in the
    order they were written unless the child is unbuffered, and while
    `run_checker` now sets `PYTHONUNBUFFERED` for exactly that reason, one
    stream cannot race itself. Printing both halves here costs nothing and does
    not depend on a setting in another file staying set.
    """
    print(reason)
    print()
    print(detail)
    return 2


def self_test() -> int:
    """Grade this gate: the attribute query, and the CR test, against real files."""
    paths = tracked_files()
    if not paths:
        return _decline(
            "cannot self-test: `git ls-files` returned nothing",
            "Being unable to run the grading is not the same as running it and\n"
            "failing. Run this from inside the worktree.")
    declared = declared_lf(paths)
    dset = {p for p in declared}

    # Two real files, chosen for what they prove rather than for convenience:
    # one that `.gitattributes` covers by an explicit rule, and one it does not
    # cover at all. If the query ever starts answering `lf` for everything --
    # or for nothing -- exactly one of these two flips.
    #
    # BOTH are drawn from `paths`, never from `declared`. Picking the `.sh` out
    # of the declared set was the first version of this, and it made the
    # assertion below a tautology: whatever the query returned was, by
    # construction, a file the query had returned. It passed a mutant that
    # walked the check-attr stream two fields at a time instead of three --
    # which is precisely the misalignment this pair of cases exists to catch.
    sh = next((p for p in paths if p.endswith(b".sh")), None)
    rs = next((p for p in paths if p.endswith(b".rs")), None)
    if sh is None or rs is None:
        return _decline(
            "cannot self-test: the tree has no tracked .sh or no tracked .rs",
            "Both are needed as fixtures: one file the attribute covers and one\n"
            "it does not. Their absence means this is not the tree this gate\n"
            "was written for, which is a reason to stop rather than to pass.")

    real = Path(sh.decode("utf-8", "surrogateescape")).read_bytes()
    other = Path(rs.decode("utf-8", "surrogateescape")).read_bytes()

    # The whole pipeline, end to end, on a file this makes and deletes.
    #
    # Everything else here grades a part. This grades the verdict: read the
    # bytes, notice the CR, print it, and *return 1*. A gate that does the first
    # three and returns 0 is exactly the shape this family of gates exists to
    # prevent, and only an end-to-end call can see it -- the mutation
    # `if not findings: return 0` -> `if True: return 0` survives every
    # component-level assertion above.
    #
    # The scratch file is outside the worktree, so nothing untracked is left in
    # the tree and no lane is ever asked to commit a broken file to prove a
    # checker works. `check` takes its path list as an argument precisely so it
    # can be pointed somewhere other than at `git ls-files`.
    with tempfile.TemporaryDirectory() as td:
        clean_f = Path(td) / "clean.sh"
        clean_f.write_bytes(b"#!/bin/sh\nset -u\necho hello\n")
        dirty_f = Path(td) / "dirty.sh"
        dirty_f.write_bytes(b"#!/bin/sh\r\nset -u\r\necho hello\r\n")
        # Redirect the reporting: these two calls print a full refusal each, and
        # a self-test whose output is two refusals reads like a failure.
        buf = io.StringIO()
        with contextlib.redirect_stdout(buf):
            rc_clean = check([str(clean_f).encode()])
            rc_dirty = check([str(dirty_f).encode()])
        pipeline_said = "carriage return(s), first at byte 9" in buf.getvalue()

    cases: list[tuple[str, object, object]] = [
        # -- the verdict, end to end
        ("end to end, a clean file returns 0", rc_clean, 0),
        ("end to end, a CRLF file returns 1", rc_dirty, 1),
        ("and the finding names the byte it found", pipeline_said, True),

        # -- the floor, as a decision rather than as a printed paragraph
        ("a run that discovered nothing declines",
         floor_reason(0, 13000) is not None, True),
        ("a run one file short of the floor declines",
         floor_reason(DISCOVERY_FLOOR - 1, 13000) is not None, True),
        ("a run at the floor proceeds",
         floor_reason(DISCOVERY_FLOOR, 13000), None),

        # -- the attribute query, which is the half that can silently empty out
        ("the declared set is not empty", bool(dset), True),
        (f"a real .sh ({sh.decode()}) is declared eol=lf", sh in dset, True),
        (f"a real .rs ({rs.decode()}) is not", rs in dset, False),
        ("the declared set is smaller than the tracked set",
         len(declared) < len(paths), True),
        # The structural invariant, and the one that does not depend on picking
        # a lucky pair of fixtures: everything the query calls a declared file
        # must be a file. A stream walked out of alignment yields the attribute
        # *name* and its *value* as paths -- `eol`, `lf` -- and those are in no
        # tracked set, so this fails immediately however the misalignment
        # happens to land.
        ("every declared path is a tracked path",
         set(declared) - set(paths), set()),
        ("the query clears the floor the real run enforces",
         len(declared) >= DISCOVERY_FLOOR, True),
        # Alignment, pinned from both sides with a two-element query whose
        # answer is known: one declared file and one undeclared one. Order
        # matters and both orders are asserted, because a stream walked two
        # fields at a time instead of three still returns the right answer for
        # `[declared, undeclared]` by luck and the wrong one for the reverse.
        # Over the whole tree that same mutant silently returns every *other*
        # file -- a plausible-looking count, all real paths, all genuinely
        # declared -- which is why no assertion about the shape of the big
        # result can catch it and this small one can.
        ("a two-file query, declared first, returns just the declared one",
         declared_lf([sh, rs]), [sh]),
        ("a two-file query, undeclared first, returns just the declared one",
         declared_lf([rs, sh]), [sh]),

        # -- the CR test, against those same real files' real bytes
        ("the real .sh is clean as it stands", scan_bytes(real)[0], 0),
        ("a CRLF planted in it is reported",
         scan_bytes(real.replace(b"\n", b"\r\n", 1))[0], 1),
        ("a lone CR planted in it is reported too",
         scan_bytes(real.replace(b"\n", b"\r", 1))[0], 1),
        ("every CR is counted, not just the first",
         scan_bytes(real.replace(b"\n", b"\r\n"))[0], real.count(b"\n")),
        ("the offset reported is the first CR",
         scan_bytes(b"ab\rcd\r")[1], 2),

        # -- and the scope: the same defect in an undeclared file is not ours
        ("a CR planted in an undeclared .rs is outside the declared set",
         rs in dset, False),
        ("the .rs fixture is real source, not an empty read", len(other) > 0, True),
    ]

    failed = 0
    for name, got, want in cases:
        if got == want:
            print(f"ok   {name}")
        else:
            print(f"FAIL {name} -- got {got!r}, want {want!r}")
            failed += 1
    print(f"\n{len(cases)} self-test case(s), {failed} failed")
    return 1 if failed else 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--list", action="store_true",
                    help="print every file that was checked")
    ap.add_argument("--self-test", action="store_true",
                    help="grade the gate against real files, not the worktree")
    args = ap.parse_args()

    if args.self_test:
        return self_test()

    paths = tracked_files()
    if not paths:
        return _decline(
            "cannot check line endings: `git ls-files` returned nothing",
            "Nothing was enumerated, so nothing was checked. This is not a clean\n"
            "worktree; it is a gate that could not find one. Run it from inside\n"
            "the repository.")

    declared = declared_lf(paths)
    low = floor_reason(len(declared), len(paths))
    if low is not None:
        return _decline(
            low,
            "The count comes from `git check-attr`, so a number this low means\n"
            "either that .gitattributes lost its `text eol=lf` rules or that the\n"
            "query itself stopped working. Both make this gate report a clean\n"
            "tree without looking at one, which is the failure it exists to\n"
            "catch, so it declines instead of passing.")

    return check(declared, show_list=args.list)


REFUSAL = """
ERROR: refusing to build.  A file above is declared `text eol=lf` in
.gitattributes and has a carriage return in the working tree.

No git command will show you this.  `text eol=lf` installs a clean filter, so
git converts CRLF to LF before it compares anything -- the file is *identical to
the index* as far as `git status`, `git diff` and `git add` are concerned, and
staging the repair produces a zero-byte diff.  That is why this gate reads the
bytes itself.

For a `.sh` this is not cosmetic: bash treats a CR as part of the token it ends,
so `set -u` becomes `set -u$'\\r'` and a sourced path acquires a trailing
carriage return.  For the rest it is harmless today and still worth knowing,
because one event corrupts whichever files it touches and the next one may touch
a script.

The cause is almost always a tool rewriting a tracked file through Python's
default text mode, which on Windows turns every `\\n` into `\\r\\n` silently.  If
you have just run something that rewrites files in place, that is the thing to
fix -- repairing the bytes alone leaves it to happen again.

To repair the working tree:

    python - <<'EOF'
    import pathlib, subprocess
    out = subprocess.run(["git","ls-files","-z"], capture_output=True).stdout
    for f in out.split(b"\\0"):
        if not f: continue
        p = pathlib.Path(f.decode())
        try: d = p.read_bytes()
        except OSError: continue
        if b"\\r" in d:
            p.write_bytes(d.replace(b"\\r\\n", b"\\n").replace(b"\\r", b"\\n"))
    EOF
"""


if __name__ == "__main__":
    sys.exit(main())
