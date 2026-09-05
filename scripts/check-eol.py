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

## Why the scope is now every *tracked* file, not every *declared* one (2026-09-04)

The paragraph above is right about what the gate is for and wrong about how far
it reaches, and the two halves were written in the same sitting. The scope it
settled on was "whatever `.gitattributes` says is `eol=lf`", on the reasoning
that reading the policy out of git beats duplicating it here as a suffix list.

That reasoning holds. The mistake is that it answers a *different question* than
the one the paragraph above poses. `.gitattributes` exists to tell git what to
do at checkout; the set of suffixes it happens to name was chosen for that, and
for nothing else. Borrowing it as this gate's scope silently makes "what do we
look at" depend on a decision taken about "what should git normalise" -- so the
gate covers a newly-declared type automatically, which is what the old paragraph
was proud of, and covers a never-declared type *never*, which it did not
consider.

Measured on lane B, 2026-09-04, while chasing an unrelated CRLF warning:

| population | files with a CR |
|---|---|
| declared `eol=lf` (what this gate used to read) | **0** |
| tracked `*.rs` and `*.toml` (declared by nothing) | 27 |
| every tracked file | **49** |

The gate reported a clean tree, correctly, against a worktree holding 49
corrupted files. `.gitattributes` names `*.sh`, `*.py`, `*.yaml`, `*.yml`,
`*.md` and `*.txt` -- not `*.rs`, not `*.toml` -- so the single largest body of
source in the repository was outside the gate's view from the day it was
written. The "size of the cause" the paragraph above wants kept visible was the
exact thing being hidden.

Widening `.gitattributes` instead is the fix lane A proposed in
`known-issues.md` -> `A-27-...`, and it is a worse one for this purpose: it is
still a suffix list, so the next text type nobody thinks to declare is invisible
again, and it conscripts a shared root file that governs three lanes' checkouts
into doing a job that belongs to one gate. Scope here, policy there. See
`design-decisions.md` §769.

The cost of looking everywhere is one measurement, not an argument: 13 908
files / 195 MB, 73 s at the old 16 threads and 44 s at 48, against ~20 s for the
~1 440 declared ones. The work is per-file antivirus interception rather than
bandwidth, so it parallelises almost perfectly up to the point where it stops
(48 and 96 threads measure the same); `READ_THREADS` is set at the knee. Twenty
extra seconds on a gate that runs first, to stop hiding two thirds of the
evidence, against the forty-seven-minute boot test the 2026-09-03 event threw
away.

## Binary files, and the one heuristic this cannot use naively

Looking at every tracked file means looking at PNGs and firmware blobs, where a
`\r` is data and a finding would be noise. The test for "is this binary" is the
same one git uses: a NUL byte anywhere in the blob.

That heuristic has already failed once in this repository, and the failure is
recorded four inches from here in `.gitattributes`: `known-issues.md` carried
two NULs ~4.5 MB in -- inside backtick spans quoting what `grep -Z` emits -- so
git called a 4.5 MB text document binary and stopped normalising it. A gate that
skipped on NUL alone would reproduce that bug exactly, and would go quiet about
precisely the file the attribute was added to protect.

So the NUL test is overridden by the attribute rather than replacing it: a file
declared `text eol=lf` is *asserted* to be text and is scanned whatever bytes it
holds. That is what `text` means, and it is the one job the attribute query is
still doing here -- narrower than before, and no longer able to hide anything by
returning an empty set, since an empty declared set now costs the NUL override
and not the entire scope.

## Why *reporting* is that wide but *refusing* is not (2026-09-04)

The paragraphs above are still the reporting policy and are unchanged. What they
got wrong is the severity, and they got it wrong by assuming the writer is
always a tool. On 2026-09-04 it was not: `todo3.txt`, the operator's own scratch
notes, picked up CRLF because a human was typing into it in a Windows editor.
That refused the build. It was repaired, and four minutes later the operator
saved again and it refused the *next* build too -- two boot tests, 187 s and
259 s, thrown away for a notes file in which nothing was wrong and nothing ever
would be. `*.txt` covers every design document in this tree, so that is not a
quirk of one file; it is every lane's build, blocked whenever the operator edits
prose on a Windows box, forever.

A gate that stops three agents' work over a byte that harms nothing gets
bypassed, and then it is not protecting the `.sh` files either. So the two
questions are now answered separately:

* **Is anything reported?** Every declared file, exactly as argued above. The
  size of the cause stays visible; nothing is hidden.
* **Does the build stop?** Only if a CR is in a file some machine *executes*
  from disk -- a `.sh`, or anything with a `#!`. See `is_run_from_disk`.

That keeps every property the 2026-09-03 event asks for. Re-run against that
event: `scripts/boot-test.sh` was among the thirteen, so the build still stops,
and the report still names all thirteen, so the size of the cause is still
there to see. What changes is only the case where the answer is "a human saved a
text file", which is not a defect and should not read as one.

Recorded in `design-decisions.md` §764.

## Cost

Reading the ~1440 declared files is ~37 MB. That is 93 s single-threaded on this
host and about a fifth of that across a thread pool, because the cost is
per-file antivirus interception rather than bandwidth (see `open-questions.md`
A-Q7, which is about exactly that tax). The pool is why this is affordable at
all; if A-Q7 is ever answered the whole thing drops into the noise.

It earns that by running *first*. The equivalent evidence exists inside
`check_shellcheck`, which sits some forty-five minutes into the sweep and only
covers `.sh` -- so a CR discovered there costs a whole boot test, which is what
it cost on 2026-09-03.

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
    0   no declared file that is executed from disk has a CR. Prose files with
        CRs are printed and do not change this code -- see the severity split
        above; `--list` and the summary line still say how many there were.
    1   a file that some machine executes from disk has a CR (the finding)
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

# Measured 2026-09-04: `git ls-files` returns 13 908 paths. 500 is well below
# any plausible shrink of the tree and far above what a broken enumeration
# returns, which is 0. The floor exists because the failure this gate is most
# likely to suffer is finding nothing and calling that clean -- the same failure
# it exists to catch elsewhere.
#
# It guards `tracked_files` and not `declared_lf`, which is the change of
# 2026-09-04: the declared set is no longer the scope, only the NUL override, so
# a query that empties out now costs one heuristic rather than the whole gate.
# There is a self-test case pinning that the declared set is still non-empty, so
# the regression is still caught -- as a graded case, at the severity it has.
DISCOVERY_FLOOR = 500

# Enough threads that the per-file antivirus round-trip overlaps, few enough
# that they are not competing for one spindle. The work is pure blocking I/O in
# the interpreter's read path, so the GIL is released throughout.
#
# 48 rather than 16 since 2026-09-04, when the scope became the whole tree.
# Measured over all 13 908 tracked files / 195 MB on this host: 73.3 s at 16,
# 44.2 s at 48, 43.7 s at 96. The curve is flat past 48, so this is the knee and
# not a number picked for looking generous.
READ_THREADS = 48


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


def is_run_from_disk(name: str, data: bytes) -> bool:
    """Whether a CR in this file changes what a *machine* does with it.

    This is the line between the two severities below, and it is drawn at
    "something executes these bytes", not at "these bytes look like code".
    Two ways for that to be true, and only two:

    * a `.sh`, because `bash` tokenises a CR as part of the word it ends, so
      `set -u` becomes `set -u$'\\r'` and a sourced path gains a trailing CR;
    * anything opening `#!`, because the kernel reads that line verbatim and a
      trailing CR becomes part of the interpreter's *path*. `/usr/bin/env
      python3\\r` is not a program that exists, and the error names a file that
      plainly does, which is among the worse diagnostics a person can be handed.

    Deliberately not keyed on `.py`. CPython decodes CRLF source correctly
    (universal newlines), so an imported module is unharmed; a `.py` that is
    *executed* is caught by its shebang, which is the property that actually
    matters. Keying on the suffix instead would refuse builds over library
    files where nothing is wrong, which is the false positive this split exists
    to remove.
    """
    if name.endswith(".sh"):
        return True
    return data.startswith(b"#!")


def is_binary(data: bytes, asserted_text: bool) -> bool:
    """Whether to treat these bytes as binary and not look for CRs in them.

    git's own test, with git's own bug fixed. A NUL byte anywhere means binary
    -- except in a file `.gitattributes` declares `text`, because that is what
    declaring `text` *means*: an assertion that overrides the guess.

    Without the override this reproduces the `known-issues.md` failure recorded
    in `.gitattributes`: two NULs 4.5 MB into a 4.5 MB text document, quoted
    from `grep -Z` output, and git silently reclassifying the whole file. The
    attribute was added precisely to stop that, so a gate that consulted the
    heuristic and not the attribute would go blind to the one file most
    expensively protected against exactly this.
    """
    return b"\0" in data and not asserted_text


def read_and_scan(
    paths: list[bytes],
    asserted_text: frozenset[bytes] = frozenset(),
) -> tuple[list[tuple[str, int, int, bool]], int, int]:
    """Scan every path, in parallel. Returns `(findings, files_read, binary)`.

    Unreadable paths are counted as read rather than skipped silently: a file
    that git tracks and this cannot open is a fact worth surfacing, and it is
    surfaced as a finding below rather than swallowed here.

    The fourth field of a finding is `is_run_from_disk`, decided here because
    this is where the bytes are in hand -- reopening the file later to look at
    two of them would double the read cost that the thread pool above exists to
    contain. Binary files are counted, not listed: the count is what shows the
    skip is skipping a plausible number of things rather than most of the tree.
    """
    findings: list[tuple[str, int, int, bool]] = []
    read = 0
    binary = 0

    def one(p: bytes) -> tuple[bool, tuple[str, int, int, bool] | None]:
        name = p.decode("utf-8", "surrogateescape")
        try:
            data = Path(name).read_bytes()
        except OSError:
            # A tracked path that will not open is not a CR finding, and
            # pretending it is would misdescribe it. It is also not nothing:
            # returning -1 as the count marks it for the caller. Fatal, because
            # unlike a CR in prose this is not a thing anyone has shown to be
            # harmless -- it is a thing nobody has been able to look at.
            return (False, (name, -1, -1, True))
        if is_binary(data, p in asserted_text):
            return (True, None)
        n, off = scan_bytes(data)
        return (False, (name, n, off, is_run_from_disk(name, data)) if n else None)

    with concurrent.futures.ThreadPoolExecutor(READ_THREADS) as pool:
        for skipped, got in pool.map(one, paths):
            read += 1
            if skipped:
                binary += 1
            elif got is not None:
                findings.append(got)
    findings.sort()
    return findings, read, binary


def floor_reason(n_found: int, n_tracked: int) -> str | None:
    """Why a run with this much discovery must decline, or `None` to proceed.

    Split out of `main` so `--self-test` can grade the decision rather than the
    printing. A floor that is computed and then not acted on is a floor that
    does not exist, and the difference is one `if`.

    `n_found` is the enumerated set since 2026-09-04 (it was the *declared*
    subset before, when that was the scope). `n_tracked` is kept as a separate
    parameter rather than folded in because the two differing is exactly what a
    future narrowing of the scope would look like, and the message should be
    able to say so.
    """
    if n_found < DISCOVERY_FLOOR:
        return (f"cannot check line endings: enumerated {n_found} of "
                f"{n_tracked} tracked files, floor is {DISCOVERY_FLOOR}")
    return None


def check(
    paths: list[bytes],
    asserted_text: frozenset[bytes] = frozenset(),
    show_list: bool = False,
) -> int:
    """Read every path, report what has a CR, and return the verdict.

    The whole pipeline below the discovery step, in one function that takes its
    input as an argument. That is what lets `--self-test` drive it end to end
    -- enumerate, read, report, decide -- against a file it made rather than
    against the tree, and so catch the one mutation that matters most here: a
    gate that finds the defect, prints it, and returns 0 anyway.
    """
    findings, read, binary = read_and_scan(paths, asserted_text)

    if show_list:
        for p in paths:
            print(f"  {p.decode('utf-8', 'surrogateescape')}")

    for name, n, off, fatal in findings:
        mark = "" if fatal else "  (not executed from disk; reported, not fatal)"
        if n < 0:
            print(f"{name}: tracked but could not be read{mark}")
        else:
            print(f"{name}: {n} carriage return(s), first at byte {off}{mark}")

    fatal = [f for f in findings if f[3]]
    print(f"\n{read} tracked file(s) read, {binary} binary and skipped, "
          f"{len(findings)} with a carriage return, {len(fatal)} of them "
          f"executed from disk")

    if not findings:
        return 0
    if not fatal:
        print(NOTICE)
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
    #
    # `.rs` is the undeclared side, and since 2026-09-04 that fact carries a
    # second meaning worth stating: it is no longer "the type this gate ignores"
    # but "the type this gate reads *without* an attribute telling it to". The
    # scope stopped depending on the attribute; only the NUL override still
    # does. If a later change ever declares `*.rs` -- see `A-27-...` in
    # `known-issues.md`, which proposes exactly that -- this fixture must move
    # to a type that is still undeclared, and the case below will say so by
    # failing rather than by silently becoming vacuous.
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
        # The severity split, as three files that differ only in the property
        # that decides it. `prose.txt` is the 2026-09-04 case -- the operator's
        # notes -- and must NOT stop a build. `hashbang` has no `.sh` suffix
        # and must, because the kernel reads its first line as a path. Without
        # that third file the split could be implemented as "refuse on .sh"
        # and pass, and a CRLF `scripts/foo` with a `#!` would sail through.
        prose_f = Path(td) / "prose.txt"
        prose_f.write_bytes(b"notes\r\nmore notes\r\n")
        hashbang_f = Path(td) / "hashbang"
        hashbang_f.write_bytes(b"#!/usr/bin/env python3\r\nprint(1)\r\n")
        # The binary override (2026-09-04), as the pair that makes it a
        # decision rather than a constant. Same bytes, same CRs, same NUL in
        # both -- the *only* difference between the two calls below is whether
        # the path is in the asserted-text set. One must be skipped and the
        # other reported, or the override is not wired to the attribute.
        blob_f = Path(td) / "blob.bin"
        blob_f.write_bytes(b"\x00\x01\r\n\x00binary\r\n")
        buf = io.StringIO()
        with contextlib.redirect_stdout(buf):
            rc_clean = check([str(clean_f).encode()])
            rc_dirty = check([str(dirty_f).encode()])
        pipeline_said = "carriage return(s), first at byte 9" in buf.getvalue()
        prose_buf = io.StringIO()
        with contextlib.redirect_stdout(prose_buf):
            rc_prose = check([str(prose_f).encode()])
            rc_hashbang = check([str(hashbang_f).encode()])
        prose_out = prose_buf.getvalue()
        bin_buf = io.StringIO()
        with contextlib.redirect_stdout(bin_buf):
            rc_blob = check([str(blob_f).encode()])
        blob_skipped = bin_buf.getvalue()
        assert_buf = io.StringIO()
        with contextlib.redirect_stdout(assert_buf):
            rc_blob_asserted = check([str(blob_f).encode()],
                                     frozenset({str(blob_f).encode()}))
        blob_asserted = assert_buf.getvalue()

    cases: list[tuple[str, object, object]] = [
        # -- the verdict, end to end
        ("end to end, a clean file returns 0", rc_clean, 0),
        ("end to end, a CRLF file returns 1", rc_dirty, 1),
        ("and the finding names the byte it found", pipeline_said, True),

        # -- the severity split (2026-09-04). The pairing is the assertion:
        # "reported" and "fatal" must come apart, or the split is not there.
        ("a CRLF text file does not stop the build", rc_prose, 0),
        ("...but is still reported, which is the whole point",
         "prose.txt: 2 carriage return(s)" in prose_out, True),
        # The marker said "(prose; ...)" until 2026-09-04, which was accurate
        # while the scope was `.md`/`.txt`/`.py` and became wrong the moment it
        # took in `*.rs`: a Rust source file with a CR is not prose, and a
        # reader told it was would reasonably stop reading. The marker names the
        # actual reason the build continued, which is the severity rule itself.
        ("...and is marked as the reason the build continued",
         "(not executed from disk; reported, not fatal)" in prose_out, True),
        ("a CRLF file with a #! stops the build even without a .sh suffix",
         rc_hashbang, 1),
        (".sh is executed from disk whatever its first bytes are",
         is_run_from_disk("x.sh", b"not a script\n"), True),
        ("a #! is executed from disk whatever its suffix is",
         is_run_from_disk("x.md", b"#!/bin/sh\n"), True),
        ("a .py without a #! is not: CPython reads CRLF source correctly",
         is_run_from_disk("x.py", b"import os\n"), False),
        ("prose is not", is_run_from_disk("notes.txt", b"hello\n"), False),

        # -- the binary override (2026-09-04). The pairing is the assertion:
        # identical bytes, and only the asserted-text set differs.
        ("a NUL-bearing file is skipped, not reported", rc_blob, 0),
        ("...and says so in the count rather than silently",
         "1 binary and skipped" in blob_skipped, True),
        ("...and its CRs are not listed",
         "carriage return(s)" in blob_skipped, False),
        ("the same bytes ARE scanned when declared text -- the known-issues.md "
         "case", "2 carriage return(s)" in blob_asserted, True),
        ("...and are counted as read rather than skipped",
         "0 binary and skipped" in blob_asserted, True),
        ("...which for a file nothing executes is reported, not fatal",
         rc_blob_asserted, 0),
        ("is_binary is the NUL test when nothing is asserted",
         is_binary(b"a\x00b", False), True),
        ("is_binary yields to the assertion",
         is_binary(b"a\x00b", True), False),
        ("is_binary does not call ordinary text binary",
         is_binary(b"hello\n", False), False),

        # -- the scope (2026-09-04): the whole tree, not the declared subset.
        # An undeclared file with a CR must now be a finding. Before this
        # change every one of these was invisible, and there were 49 of them.
        ("a CR in an undeclared type is now in scope",
         scan_bytes(other.replace(b"\n", b"\r\n", 1))[0], 1),
        ("the scope passed to check is the tracked set, not the declared one",
         len(paths) > len(declared), True),

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
        # The declared set no longer gates the run, so this is not "clears the
        # floor" any more -- it is a standing check that the attribute query
        # still answers, kept at the severity it now has. If it empties out,
        # NUL-bearing text files start being read as binary and nothing else
        # changes; that is a real regression and a much smaller one than
        # before, and this is where it is caught.
        ("the attribute query still returns a plausible set",
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

        # -- the declared set's one remaining job: overriding the NUL guess.
        # It is no longer the scope, so this says "outside the *assertion*",
        # not "outside the gate". A `.rs` full of CRs is now a finding; what
        # being undeclared costs it is only that a NUL inside it would be
        # believed.
        ("a .rs is outside the asserted-text set", rs in dset, False),
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

    low = floor_reason(len(paths), len(paths))
    if low is not None:
        return _decline(
            low,
            "The count comes from `git ls-files`, so a number this low means the\n"
            "enumeration itself stopped working. That makes this gate report a\n"
            "clean tree without looking at one, which is the failure it exists\n"
            "to catch, so it declines instead of passing.")

    # The declared set is no longer the scope -- only the override that keeps a
    # NUL-bearing text file from being mistaken for a binary one. A failure here
    # therefore costs one heuristic rather than the whole gate, so it is not
    # worth declining over; the self-test grades it instead.
    return check(paths, frozenset(declared_lf(paths)), show_list=args.list)


NOTICE = """
NOTE: the tracked file(s) above have carriage returns in the working tree.  None
of them is executed from disk -- no `.sh`, nothing with a `#!` -- so nothing is
broken by this and the build goes ahead.  It is printed anyway, every run,
because a CR is evidence about whatever wrote the file and that is worth knowing
even when it costs nothing today.

Being listed here does NOT mean the file is declared `text eol=lf`.  Since
2026-09-04 this gate reads every tracked file, because the declared set turned
out to exclude `*.rs` and `*.toml` -- so it was reporting a clean tree over 49
corrupted files.  A `.rs` or a `.toml` in the list is therefore expected and is
not a `.gitattributes` violation; it is the same evidence about the same
writer, from the part of the tree that used to be invisible.

The commonest cause is benign and needs no action: a person editing a `.txt` or
`.md` in a Windows editor, which saves CRLF.  Git normalises it away on commit,
so it never reaches history.

The cause worth acting on is a *tool* rewriting tracked files through Python's
default text mode.  Tell the two apart by the shape: one file, growing, that
someone is plainly editing is a person; several files at once, or a file no
human would open, is a tool -- and a tool that does this will reach a `.sh`
eventually, which is the run that stops the build.

To repair the bytes, see the command in the refusal text below this gate's
source, or just re-save the file with LF endings.
"""

REFUSAL = """
ERROR: refusing to build.  A file above is executed from disk -- a `.sh`, or
something opening `#!` -- and has a carriage return in the working tree.

No git command will show you this.  With `core.autocrlf=input`, and again under
`text eol=lf`, git converts CRLF to LF before it compares anything -- the file
is *identical to the index* as far as `git status`, `git diff` and `git add` are
concerned, and staging the repair produces a zero-byte diff.  That is why this
gate reads the bytes itself.

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
