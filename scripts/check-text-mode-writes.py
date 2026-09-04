#!/usr/bin/env python3
"""Refuse any text-mode write under `scripts/` that does not pass `newline=`.

On Windows, a file opened in text mode translates every `\\n` the program writes
into `\\r\\n`. Silently. `open(p, "w")`, `Path.write_text(...)` and
`os.fdopen(fd, "w")` all do it, it is the documented default, and nothing warns.
So the most obvious spelling of "write this text to this file" is wrong by
default on the platform this tree is developed on.

That is not a hypothetical. On 2026-09-03 `scripts/boot-test.sh` and twelve
other files declared `text eol=lf` sat wholly CRLF in the worktree while every
git command reported a clean tree -- see `check-eol.py`'s docstring for why git
cannot see it, and `known-issues.md` ->
`TD-A-A-DECLARED-LF-FILES-WERE-CRLF-ON-DISK-AND-GIT-SAID-NOTHING` for the
incident. `check-eol.py` catches the *consequence*, forty-five minutes cheaper
than `shellcheck` did. This gate catches the *cause*.

## Why blanket, and not scoped to writes that reach a tracked file

Adding `newline=""` at the sites that today write into the repository is correct
at each of those sites and protects nothing at the next one. Three reasons the
scope is every text-mode write instead:

- **The destination usually cannot be resolved statically.** Most of these write
  to a path computed at runtime. A gate that only fires when it can prove the
  target is tracked is a gate that mostly cannot prove anything.
- **A temp-dir write today is a repo write tomorrow**, the moment someone reuses
  the helper. The 2026-09-03 corruption came from a writer nobody had classified
  as dangerous.
- **A CRLF fixture is a real defect even when nothing tracked is touched.** A
  self-test that writes its own input in text mode gets different bytes on
  Windows than on Linux, so it grades a different thing on each platform -- and
  that is precisely the class of bug this tree keeps finding: a check that
  reports "pass" because it was handed something other than what it meant to
  look at.

`newline=""` costs nothing anywhere, so there is no scope worth arguing about.

## What counts as a finding

A call is a finding when the mode it opens in is a **text write** and no
`newline=` keyword is present. Modes are resolved statically:

| shape | mode comes from | default when absent |
|---|---|---|
| `open(p, m)`, `io.open(p, m)`, `os.fdopen(fd, m)` | positional 1, or `mode=` | `"r"` -- a read |
| `p.open(m)` (`Path.open`) | positional 0, or `mode=` | `"r"` -- a read |
| `NamedTemporaryFile(m)`, `TemporaryFile(m)` | positional 0, or `mode=` | `"w+b"` -- **binary** |
| `p.write_text(...)` | n/a, always text | -- |

So `open(p)` is not a finding (it reads), `open(p, "wb")` is not (binary), and
`NamedTemporaryFile(suffix=".txt")` is not (its default is binary, unlike
everything else here) -- while `open(p, "w")`, `open(p, "a")`, `p.open("w")`,
`io.open(p, "x")` and any bare `write_text` are.

**A mode this cannot read statically is also a finding.** If the mode is a
variable, an f-string or a conditional, the gate cannot tell a text write from a
binary one, and the honest response is to say so rather than to assume the
harmless case. A gate that shrugs at what it cannot decide is the exact failure
this family of gates exists to prevent -- it reports no findings, which reads
like a clean tree. The tree has zero such sites today (measured 2026-09-04), so
the rule costs nothing now and stays correct if one appears; the fix is to make
the mode a literal, or to split the call.

**`newline=None` is rejected, not accepted.** It is the broken default written
out longhand: passing it explicitly changes nothing about the bytes. Accepting
it would let a one-word edit turn any finding green while leaving the defect in
place, which is worse than not having the gate.

## Why reads are not graded

107 of the tree's text-mode reads pass no `newline=` either, and almost all of
them are correct: reading in text mode *normalises* CRLF to `\\n`, which is
usually what a reader wants. It only becomes a defect in a read-modify-write
round trip -- read normalises to `\\n`, write turns every one back into `\\r\\n`,
so a file flips to CRLF whenever the script changes anything at all. That was
`strip-workspace-sections.py`, deleted 2026-09-04.

But the round trip is caught at its *write* end, which this gate already grades.
Gating the read end too would add ~107 findings that are each individually fine
to buy no coverage the write rule does not already give, and a gate with a
hundred findings nobody believes is a gate that gets bypassed.

## Scope is `scripts/`, because that is this lane's tree

Python outside `scripts/` -- `kernel/ada/*.py`, lane B's `services/*/build.py`,
lane C's app tooling -- has the same defect and is not lane A's to edit. Lane B
already met the consequence of it (see
`B-THE-FIXTURE-STAMP-HASHED-WORKTREE-BYTES-SO-IT-DID-NOT-SURVIVE-A-CHECKOUT`,
2026-08-16: 70 CRLF files, every one a `services/*/build.py`) and fixed the
hash rule rather than the writer, so the writer is still running. `--all`
surveys the whole tree without grading it, which is the evidence a cross-lane
request would need.

Exit codes:
    0   every text-mode write under scripts/ passes an explicit newline=
    1   at least one does not (the finding)
    2   could not look: not a git worktree, a file that will not parse, or
        fewer files/sites found than the floor

Usage:
    python scripts/check-text-mode-writes.py             # grade scripts/
    python scripts/check-text-mode-writes.py --list      # also list clean sites
    python scripts/check-text-mode-writes.py --all       # survey the whole tree
    python scripts/check-text-mode-writes.py --self-test # grade the gate
"""

from __future__ import annotations

import argparse
import ast
import io
import contextlib
import re
import subprocess
import sys

# Measured 2026-09-04: 169 tracked `*.py` under `scripts/`, carrying 197
# text-mode write sites (110 already compliant, 87 not). Two floors, because
# they fail differently and only one of them catches the mutation that matters.
#
# FILE_FLOOR catches a broken *enumeration*: `git ls-files` renamed, the path
# prefix mistyped, the suffix filter inverted. That drops files to 0.
#
# SITE_FLOOR catches a broken *detector*, which is the likelier and quieter
# failure: a mode-resolution rule that stops recognising `"w"`, an AST walk that
# stops matching `Call` nodes, a keyword lookup that goes to the wrong field.
# All of those leave the file count untouched at 169 and report a clean tree.
# The site floor counts every text write the gate *understood* -- compliant ones
# included -- so it measures comprehension rather than luck: a tree that
# genuinely fixed all 87 findings still clears it at 197.
FILE_FLOOR = 100
SITE_FLOOR = 120

# A mode string, as `open` accepts it. Used to pick the mode out of a call's
# positional arguments without having to know which index it sits at -- the
# index differs between `open(path, mode)` and `Path.open(mode)`, and a path
# literal never spells `"w"`.
MODE_RE = re.compile(r"[rwxab+tU]{1,4}\Z")

# `(name, default_mode)`. The default is what the call opens in when no mode is
# given, and it is not uniform: `tempfile`'s openers default to *binary*, every
# other one here defaults to a read. Getting that backwards would make ~51 bare
# `open(p)` reads look like findings, or hide a bare `NamedTemporaryFile` write.
OPENERS = {
    "open": "r",
    "fdopen": "r",
    "NamedTemporaryFile": "w+b",
    "TemporaryFile": "w+b",
    "SpooledTemporaryFile": "w+b",
}

# Receivers for which `X.open(...)` has the *builtin* signature -- path first,
# mode second -- rather than `Path.open`'s mode-first one. `io.open` is the
# builtin. This only matters for spotting a non-literal mode, since a literal
# one is found by shape; it is spelled out anyway so the asymmetry is on the
# record rather than in someone's head.
BUILTIN_SHAPED_RECEIVERS = {"io", "codecs", "os", "gzip", "bz2", "lzma"}

UNKNOWN_MODE = "<computed>"


def _git(args: list[str]) -> subprocess.CompletedProcess:
    return subprocess.run(["git", *args], capture_output=True, check=False)


def tracked_py(prefix: str | None = "scripts") -> list[str]:
    """Every tracked `*.py`, under `prefix` if given.

    Asking git rather than walking the filesystem keeps build output, virtualenvs
    and another lane's scratch files out of the subject set, and makes the
    subject set the same on every host.
    """
    args = ["ls-files", "-z"]
    if prefix:
        args.append(prefix)
    r = _git(args)
    if r.returncode != 0:
        return []
    out = []
    for raw in r.stdout.split(b"\0"):
        if raw.endswith(b".py"):
            out.append(raw.decode("utf-8", "surrogateescape"))
    return sorted(out)


def _func_name(call: ast.Call) -> tuple[str | None, str | None]:
    """`(name, receiver)` for a call, or `(None, None)` if it is neither shape.

    The receiver is the source text of whatever the attribute hangs off, so
    `io.open(...)` gives `("open", "io")` and `p.open(...)` gives
    `("open", "p")`.
    """
    fn = call.func
    if isinstance(fn, ast.Name):
        return fn.id, None
    if isinstance(fn, ast.Attribute):
        recv = fn.value.id if isinstance(fn.value, ast.Name) else None
        return fn.attr, recv
    return None, None


def _keyword(call: ast.Call, name: str) -> ast.expr | None:
    for k in call.keywords:
        if k.arg == name:
            return k.value
    return None


def resolve_mode(call: ast.Call, name: str, receiver: str | None) -> str:
    """The mode this call opens in: a literal, its default, or `UNKNOWN_MODE`.

    A `mode=` keyword wins outright. Failing that, the mode is looked for among
    the positional arguments *by shape* rather than by index, because the index
    is not the same for every spelling: `open(path, "w")` puts it second and
    `Path.open("w")` puts it first. A path literal never matches `MODE_RE`, so
    the shape test is unambiguous in practice -- and where it is not (two
    mode-shaped literals, which would be a call nobody wrote on purpose) the
    answer is `UNKNOWN_MODE`, which is a finding rather than a guess.
    """
    kw = _keyword(call, "mode")
    if kw is not None:
        if isinstance(kw, ast.Constant) and isinstance(kw.value, str):
            return kw.value
        return UNKNOWN_MODE

    lits = [a.value for a in call.args
            if isinstance(a, ast.Constant) and isinstance(a.value, str)
            and MODE_RE.fullmatch(a.value)]
    if len(lits) == 1:
        return lits[0]
    if len(lits) > 1:
        return UNKNOWN_MODE

    # No literal mode anywhere. Either the call takes the default, or the mode
    # is an expression this cannot read. Which one depends on whether anything
    # is sitting in the slot the mode would occupy.
    idx = 1 if (receiver is None or receiver in BUILTIN_SHAPED_RECEIVERS) else 0
    if name in ("NamedTemporaryFile", "TemporaryFile", "SpooledTemporaryFile"):
        idx = 0
    if len(call.args) > idx:
        return UNKNOWN_MODE
    return OPENERS.get(name, "r")


def is_text_write(mode: str) -> bool:
    """Does `mode` open a file for writing, in text mode?

    `"b"` anywhere makes it binary and therefore safe -- binary is the mode that
    writes exactly the bytes it is given. `"+"` counts as writing: `"r+"` opens
    an existing file for update, and an update in text mode CRLF-ifies whatever
    it writes just as surely as `"w"` does.
    """
    if mode == UNKNOWN_MODE:
        return False
    if "b" in mode:
        return False
    return any(c in mode for c in "wax+")


def newline_verdict(call: ast.Call) -> str | None:
    """`None` if this call's `newline=` is acceptable, else why it is not."""
    kw = _keyword(call, "newline")
    if kw is None:
        return "no newline= argument"
    if isinstance(kw, ast.Constant) and kw.value is None:
        # The default, written out longhand. It changes no bytes, so accepting
        # it would let a one-word edit turn any finding green.
        return "newline=None is the platform default spelled out, not a choice"
    return None


class Finding:
    """One call site, and what is wrong with it."""

    __slots__ = ("path", "line", "col", "call", "mode", "why")

    def __init__(self, path: str, line: int, col: int, call: str,
                 mode: str, why: str) -> None:
        self.path, self.line, self.col = path, line, col
        self.call, self.mode, self.why = call, mode, why

    def __repr__(self) -> str:
        return (f"{self.path}:{self.line}:{self.col}: {self.call}"
                f"(mode={self.mode!r}) -- {self.why}")

    def key(self) -> tuple:
        return (self.path, self.line, self.col)


def analyse(source: bytes, path: str) -> tuple[list[Finding], int]:
    """`(findings, text_write_sites_seen)` for one file's source.

    The second number is the floor's input and is deliberately *not* the number
    of findings: it counts every text write the analysis understood, compliant
    or not, so it stays roughly constant as the findings are fixed. A count that
    fell to zero as the tree got clean could not distinguish a clean tree from a
    detector that stopped working.
    """
    tree = ast.parse(source, filename=path)
    findings: list[Finding] = []
    seen = 0

    for node in ast.walk(tree):
        if not isinstance(node, ast.Call):
            continue
        name, recv = _func_name(node)
        if name is None:
            continue

        if name == "write_text":
            seen += 1
            why = newline_verdict(node)
            if why is not None:
                findings.append(Finding(path, node.lineno, node.col_offset,
                                        "write_text", "text", why))
            continue

        if name not in OPENERS:
            continue

        mode = resolve_mode(node, name, recv)
        if mode == UNKNOWN_MODE:
            findings.append(Finding(
                path, node.lineno, node.col_offset, name, mode,
                "mode is not a literal, so this cannot be told from a binary "
                "write -- make it a literal or split the call"))
            continue
        if not is_text_write(mode):
            continue
        seen += 1
        why = newline_verdict(node)
        if why is not None:
            findings.append(Finding(path, node.lineno, node.col_offset,
                                    name, mode, why))

    return findings, seen


def scan(paths: list[str]) -> tuple[list[Finding], int, list[tuple[str, str]]]:
    """Analyse every path. Returns `(findings, sites_seen, unparseable)`."""
    findings: list[Finding] = []
    seen = 0
    bad: list[tuple[str, str]] = []
    for p in paths:
        try:
            with open(p, "rb") as fh:
                src = fh.read()
        except OSError as exc:
            bad.append((p, f"cannot read: {exc}"))
            continue
        try:
            f, n = analyse(src, p)
        except SyntaxError as exc:
            # Not a finding, and not nothing. A tracked `.py` this cannot parse
            # is a file the gate did not grade, and saying "clean" about a file
            # you could not read is the failure mode this whole family of gates
            # exists to prevent.
            bad.append((p, f"cannot parse: {exc}"))
            continue
        findings.extend(f)
        seen += n
    findings.sort(key=Finding.key)
    return findings, seen, bad


def floor_reason(n_files: int, n_sites: int) -> str | None:
    """Why a run with this much discovery must decline, or `None` to proceed."""
    if n_files < FILE_FLOOR:
        return (f"cannot check text-mode writes: found only {n_files} tracked "
                f"python files, floor is {FILE_FLOOR}")
    if n_sites < SITE_FLOOR:
        return (f"cannot check text-mode writes: recognised only {n_sites} "
                f"text-mode write sites, floor is {SITE_FLOOR}")
    return None


def report(findings: list[Finding], seen: int, bad: list[tuple[str, str]],
           n_files: int) -> int:
    """Print the verdict and return the exit status.

    Takes its input as arguments rather than doing its own discovery, so
    `--self-test` can drive the whole pipeline -- analyse, report, decide --
    over source it made up, and so catch the one mutation that matters most:
    a gate that finds the defect, prints it, and returns 0 anyway.
    """
    for p, why in bad:
        print(f"{p}: {why}")
    for f in findings:
        print(f"{f.path}:{f.line}: {f.call}(mode={f.mode!r}) -- {f.why}")

    print(f"\n{n_files} file(s), {seen} text-mode write site(s), "
          f"{len(findings)} without an explicit newline=, "
          f"{len(bad)} not graded")

    if not findings and not bad:
        return 0
    print(REFUSAL)
    return 1


def _decline(reason: str, detail: str) -> int:
    """Exit 2 with the reason as the FIRST line, and everything on one stream.

    `run_checker` quotes the first line of the merged log as the reason a gate
    declined; two streams into one file do not reliably arrive in write order.
    Same convention as `check-eol.py`.
    """
    print(reason)
    print()
    print(detail)
    return 2


# Fixture source for the self-test. Every line is a rule this gate makes, so a
# rule that stops being enforced shows up as a named failure rather than as a
# number that drifted.
_CLEAN_SRC = b'''
import io, os, tempfile
from pathlib import Path
p = Path("x")
open("a.txt", "w", newline="")            # explicit, the point of the gate
open("b.txt", "w", newline="\\n")          # explicit and not "" -- also fine
open("c.txt", "a", newline="")
open("d.txt", "wb")                        # binary writes exactly what it is given
open("e.txt", "rb")
open("f.txt", "r")
open("g.txt")                              # default is a read
p.open("w", newline="")
io.open("h.txt", "w", newline="")
os.fdopen(3, "w", newline="")
p.write_text("s", newline="")
tempfile.NamedTemporaryFile(suffix=".t")   # tempfile defaults to BINARY
tempfile.NamedTemporaryFile("wb")
'''

_DIRTY_SRC = b'''
import io, os, tempfile
from pathlib import Path
p = Path("x")
open("a.txt", "w")
open("b.txt", "a")
open("c.txt", "x")
open("d.txt", "r+")
open("e.txt", mode="w")
p.open("w")
io.open("f.txt", "w")
os.fdopen(3, "w")
p.write_text("s")
tempfile.NamedTemporaryFile(mode="w", suffix=".t")
open("g.txt", "w", newline=None)
'''


def self_test() -> int:
    """Grade this gate: the mode rules, the verdict, and the attachment."""
    paths = tracked_py("scripts")
    if not paths:
        return _decline(
            "cannot self-test: `git ls-files scripts` returned no python",
            "Being unable to run the grading is not the same as running it and\n"
            "failing. Run this from inside the worktree.")

    clean_f, clean_seen = analyse(_CLEAN_SRC, "<clean>")
    dirty_f, dirty_seen = analyse(_DIRTY_SRC, "<dirty>")

    # The verdict, end to end. `report` returning 0 on a non-empty finding list
    # is the mutation that survives every component assertion above, and only a
    # real call can see it. Output is captured because a self-test whose log is
    # two full refusals reads like a failure.
    buf = io.StringIO()
    with contextlib.redirect_stdout(buf):
        rc_clean = report([], clean_seen, [], 1)
        rc_dirty = report(dirty_f, dirty_seen, [], 1)
        rc_unparseable = report([], 300, [("x.py", "cannot parse: boom")], 1)
    said = buf.getvalue()

    # Attachment: the gate must still be looking at the real tree, and must
    # still understand what it finds there. `analyse` is run over a real
    # tracked script's real bytes -- not over a string this file invented --
    # because a detector that only works on the fixture is the exact thing a
    # fixture cannot reveal.
    real_findings, real_seen, real_bad = scan(paths)
    self_path = "scripts/check-text-mode-writes.py"
    me = [p for p in paths if p == self_path]

    def modes(src: bytes) -> list[str]:
        return sorted(f"{f.call}:{f.mode}" for f in analyse(src, "<t>")[0])

    def one(src: str) -> list[Finding]:
        return analyse(src.encode(), "<t>")[0]

    cases: list[tuple[str, object, object]] = [
        # -- the verdict, end to end
        ("end to end, no findings returns 0", rc_clean, 0),
        ("end to end, findings return 1", rc_dirty, 1),
        ("end to end, an ungraded file returns 1 even with no findings",
         rc_unparseable, 1),
        ("the report names a finding's file and line",
         "<dirty>:5:" in said, True),

        # -- every mode rule, as a rule and not as a total
        ("a bare open(p,'w') is a finding", len(one("open(p,'w')")), 1),
        ("open(p,'w',newline='') is not", len(one("open(p,'w',newline='')")), 0),
        ("open(p,'w',newline='\\n') is not",
         len(one("open(p,'w',newline='\\n')")), 0),
        ("newline=None is a finding, not an escape",
         len(one("open(p,'w',newline=None)")), 1),
        ("a bare open(p,'wb') is not a finding", len(one("open(p,'wb')")), 0),
        ("a bare open(p,'rb') is not", len(one("open(p,'rb')")), 0),
        ("a bare open(p,'r') is not", len(one("open(p,'r')")), 0),
        ("a bare open(p) is not -- the default is a read",
         len(one("open(p)")), 0),
        ("open(p,'a') is a finding", len(one("open(p,'a')")), 1),
        ("open(p,'x') is a finding", len(one("open(p,'x')")), 1),
        ("open(p,'r+') is a finding -- update is a write",
         len(one("open(p,'r+')")), 1),
        ("open(p,'rb+') is not -- binary", len(one("open(p,'rb+')")), 0),
        ("mode= as a keyword is read", len(one("open(p,mode='w')")), 1),
        ("mode= as a keyword, binary, is read too",
         len(one("open(p,mode='wb')")), 0),

        # -- the shape asymmetry: Path.open puts the mode first, open() second
        ("Path.open('w') is found with the mode in slot 0",
         len(one("p.open('w')")), 1),
        ("io.open(p,'w') is found with the mode in slot 1",
         len(one("io.open(p,'w')")), 1),
        ("Path.open('rb') is not a finding", len(one("p.open('rb')")), 0),
        ("os.fdopen(fd,'w') is a finding", len(one("os.fdopen(fd,'w')")), 1),
        ("a bare os.fdopen(fd) is not", len(one("os.fdopen(fd)")), 0),

        # -- tempfile, whose default is the opposite of everything else's
        ("a bare NamedTemporaryFile is not a finding -- it defaults to binary",
         len(one("tempfile.NamedTemporaryFile(suffix='.t')")), 0),
        ("NamedTemporaryFile(mode='w') is a finding",
         len(one("tempfile.NamedTemporaryFile(mode='w')")), 1),
        ("NamedTemporaryFile('w') is a finding -- mode is positional 0",
         len(one("tempfile.NamedTemporaryFile('w')")), 1),

        # -- write_text, which is always text and so is always graded
        ("a bare write_text is a finding", len(one("p.write_text(s)")), 1),
        ("write_text(newline='') is not",
         len(one("p.write_text(s,newline='')")), 0),
        ("read_text is not graded", len(one("p.read_text()")), 0),

        # -- the undecidable case, which must be a finding and not a shrug
        ("a computed mode is a finding", len(one("open(p,m)")), 1),
        ("an f-string mode is a finding", len(one("open(p,f'{m}')")), 1),
        ("a computed mode= keyword is a finding", len(one("open(p,mode=m)")), 1),
        ("a computed mode is reported as undecidable, not as a missing newline",
         one("open(p,m)")[0].mode, UNKNOWN_MODE),
        ("a computed mode is a finding even with newline= present",
         len(one("open(p,m,newline='')")), 1),

        # -- the fixtures as wholes, so a rule that starts double-counting shows
        # These two totals are hand-counted from the fixtures above and are the
        # only assertions here that a *double-counting* rule can fail -- every
        # per-rule case below uses a one-line source, where counting twice and
        # counting once are both "a finding". Clean: the three `open`s with
        # newline=, `p.open`, `io.open`, `os.fdopen`, `write_text` -- 7 graded,
        # 0 wrong. Dirty: w, a, x, r+, mode=w, Path.open, io.open, fdopen,
        # write_text, NamedTemporaryFile(mode=w), newline=None -- 11.
        ("the clean fixture has no findings", len(clean_f), 0),
        ("the clean fixture still has text writes to grade", clean_seen, 7),
        ("the dirty fixture's findings are exactly its lines", len(dirty_f), 11),
        ("the dirty fixture's findings are all distinct sites",
         len({f.key() for f in dirty_f}), len(dirty_f)),

        # -- attachment: the real tree, not the fixture
        ("the subject set is not empty", bool(paths), True),
        ("this gate is in its own subject set", me, [self_path]),
        ("every tracked .py parsed", real_bad, []),
        ("the real tree clears the file floor", len(paths) >= FILE_FLOOR, True),
        ("the real tree clears the site floor", real_seen >= SITE_FLOOR, True),
        ("the floor declines a run that found no files",
         floor_reason(0, 9999) is not None, True),
        ("the floor declines a run that recognised no sites",
         floor_reason(9999, 0) is not None, True),
        ("the floor passes the real tree",
         floor_reason(len(paths), real_seen), None),
        # The detector must work on real source, not only on strings this file
        # wrote. Its own body contains `open(p, "rb")` and no text write, which
        # is a fact about a real file that a fixture cannot stand in for.
        ("this gate's own source is clean",
         [f for f in real_findings if f.path == self_path], []),
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
    ap.add_argument("--all", action="store_true",
                    help="survey every tracked *.py in the tree, not just "
                         "scripts/ -- reports, does not grade")
    ap.add_argument("--self-test", action="store_true",
                    help="grade the gate, not the tree")
    args = ap.parse_args()

    if args.self_test:
        return self_test()

    if args.all:
        paths = tracked_py(None)
        findings, seen, bad = scan(paths)
        by_top: dict[str, int] = {}
        for f in findings:
            by_top[f.path.split("/")[0]] = by_top.get(f.path.split("/")[0], 0) + 1
        for p, why in bad:
            print(f"{p}: {why}")
        for f in findings:
            print(f"{f.path}:{f.line}: {f.call}(mode={f.mode!r}) -- {f.why}")
        print(f"\n{len(paths)} file(s), {seen} text-mode write site(s), "
              f"{len(findings)} without an explicit newline=")
        for top in sorted(by_top):
            print(f"  {by_top[top]:4}  {top}/")
        print("\nSurvey only -- this does not grade. Findings outside scripts/ "
              "belong to another lane;\nfile a request rather than editing them.")
        return 0

    paths = tracked_py("scripts")
    if not paths:
        return _decline(
            "cannot check text-mode writes: `git ls-files scripts` found no python",
            "Nothing was enumerated, so nothing was checked. This is not a clean\n"
            "tree; it is a gate that could not find one. Run it from inside the\n"
            "repository.")

    findings, seen, bad = scan(paths)
    low = floor_reason(len(paths), seen)
    if low is not None:
        return _decline(
            low,
            "The counts come from `git ls-files` and from this gate's own AST\n"
            "walk, so numbers this low mean either that the enumeration stopped\n"
            "finding files or that the analysis stopped recognising writes.\n"
            "Both make this gate report a clean tree without having graded one,\n"
            "which is the failure it exists to catch, so it declines instead of\n"
            "passing.")

    if args.list:
        for p in paths:
            print(f"  {p}")

    return report(findings, seen, bad, len(paths))


REFUSAL = """
ERROR: refusing to build.  A call above opens a file for writing in TEXT mode
without saying what it wants line endings to be.

On Windows that translates every `\\n` it writes into `\\r\\n`, silently, and no
git command will show you the result: a file declared `text eol=lf` that is CRLF
on disk is *identical to the index* as far as git is concerned.  On 2026-09-03
that cost a whole boot test -- boot-test.sh and twelve other files sat wholly
CRLF while `git status` was clean, and shellcheck found it forty-five minutes in.

The fix at each site is one keyword:

    open(p, "w", newline="")          # LF on every platform
    p.write_text(s, newline="")
    os.fdopen(fd, "w", newline="")

Use `newline=""` unless you specifically want something else; `newline="\\n"` is
also accepted.  `newline=None` is NOT -- it is the broken default written out
longhand and changes nothing.

If the file is not text, say so instead: `"wb"` writes exactly the bytes it is
given and is not graded here.

If the mode is reported as '<computed>', this gate could not tell a text write
from a binary one.  Make the mode a literal, or split the call in two.
"""


if __name__ == "__main__":
    sys.exit(main())
