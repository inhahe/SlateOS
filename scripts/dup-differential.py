#!/usr/bin/env python3
"""Decide a duplicated binary pair by BEHAVIOUR, against GNU coreutils.

# Why this exists

design-decisions.md §1005 (Operator) settles that `coreutils` is the one home:
for each of the duplicated binary names, the better half survives inside it and
the duplicate crate is deleted. `scripts/dup-bins-survey.py` ranks the pairs by
size and by which command-line options each side mentions, and its own entry in
`known-issues.md` is explicit that it is **triage, not a verdict** -- every pair
is read before either copy goes.

Reading is not always enough. `fold` was retired on 2026-09-11 and the survey
called it "close -- read both": 483 standalone lines against coreutils' 874,
and **no option unique to either side**, so an options diff had nothing to say
at all. A behavioural differential said everything -- coreutils agreed with GNU
on 18 of 18 cases and the standalone on 14, and none of the four failures was a
missing option:

    -w 0        GNU and coreutils REFUSE; the standalone silently folded
                to one character per line
    legacy -5   accepted by both, rejected by the standalone
    multibyte   three `e-acute` at -w 4: broken after two, or not at all
    a CR        a column reset, or a line break

That is the shape this tool exists to find: not missing features, which
reading finds, but *wrong answers*, which it does not.

# USE `scripts/<name>-diff.sh` INSTEAD WHEN ONE EXISTS

There are 59 of those, and they cover **half the remaining duplicate pairs**:
awk, cmp, comm, dd, df, du, expand, join, paste, sed, sort, split, tar, tee,
tsort, uniq, wc, xargs. They are better than this file at the same job and I
did not look for them before writing it.

* They carry far more cases -- `nl-diff.sh` has 222 against the 43 here.
* They compare through `od -An -c`, so whitespace is exact. That matters: the
  defect that decided `nl` was a literal tab where GNU pads with spaces, and a
  comparison that collapsed whitespace "would agree with almost every wrong
  implementation" (its words).
* They run BOTH sides inside WSL under `LC_ALL=C.UTF-8`, because the Windows
  host's own coreutils are MSYS2's -- a Cygwin derivative whose `getopt` words
  every diagnostic differently, so a harness pointed at it certifies sentences
  no GNU/Linux system prints.
* **They are parameterised**: `OURS=/path/to/binary ./scripts/nl-diff.sh`. So
  the standalone half of a pair can be fed to them directly, which is exactly
  the question this file was written for.

This file is for the OTHER half -- cal, chown, date, diff, env, free,
hostname, kill, logger, patch, ps, sha256sum, stat, strings, uname, uptime --
where no such harness exists. Writing one `-diff.sh` per name would be better
still; this is the cheaper thing that covers all of them.

# THE REFERENCE HERE IS WEAKER THAN THEIRS, AND THAT IS THIS FILE'S REAL LIMIT

`scripts/diff-wsl.sh` **builds GNU coreutils 9.4 from source** to compare
against, because WSL's installed coreutils is Ubuntu's `9.4-3ubuntu6.1` and
carries behavioural patches -- so a green run against it "certifies agreement
with Debian rather than with GNU" (design-decisions.md §726).

This file runs `wsl -e <name>`, which is that patched build. So a count here is
agreement with Ubuntu's coreutils, not with GNU, and the distinction is real.

Why the verdicts it has produced still stand: in every pair so far the
`coreutils` side scored **100%** against this same reference while the
standalone scored 52-88%, and the deciding differences were structural -- `seq`
counting backwards where GNU prints nothing, `nl -l` printing nothing at all,
`tr` accepting an extra operand. Those are not distribution patches, and a
reference that were systematically wrong could not have produced a perfect
score for one side. Internal consistency is not the same as a built reference,
which is why this paragraph exists rather than a reassurance.

**The right fix is a `<name>-diff.sh` per name, on `diff-wsl.sh`.** That gets
the built reference, both sides under one `argv[0]`, and `od -An -c`. Until
someone writes those eighteen, this is what covers them, and its number should
be read as "does the standalone disagree with a real coreutils" rather than as
a conformance score.

Our own side runs as a Windows binary, which is sound for a stdin filter and is
a limit worth remembering for anything that touches paths or line endings.

# What it does

For a name with both a `userspace/<name>` crate and a `coreutils` bin, it
builds each side, runs both plus the host's GNU implementation over a list of
cases, and reports how often each agrees with GNU on **stdout, stderr and exit
status together**.

It does not delete anything and does not decide anything. It produces the
evidence a person deletes on.

# Why the two binaries are captured separately

Both sides build a binary with the same file name into the same target
directory, so the second `cargo build` silently overwrites the first -- which
is the non-determinism §1005 describes, reproducing locally in seconds. Each is
copied aside immediately after its own build.

# The cases

`scripts/dup-differential-cases/<name>.txt`, one case per line:

    <label> | <stdin, with \\n \\t \\r \\xNN escapes> | <argv...>

A case list is a claim about what the utility is for, so it lives beside the
tool as data rather than being buried in it, and a reviewer can see what was
and was not asked. An absent file is an error rather than an empty run: zero
cases would otherwise report perfect agreement.
"""

from __future__ import annotations

import os
import shlex
import shutil
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import selftestflag  # noqa: E402

CASES_DIR = Path(__file__).resolve().parent / "dup-differential-cases"
TARGET = "x86_64-pc-windows-gnu"


def unescape(s: str) -> bytes:
    """`\\n`, `\\t`, `\\r`, `\\0`, `\\\\` and `\\xNN` -> bytes.

    Bytes and not text: the inputs that separate these implementations are
    routinely not UTF-8 -- a lone 0x80, a CR in the middle of a line -- and a
    case list that could only express text would omit exactly the cases worth
    running.
    """
    out = bytearray()
    i = 0
    while i < len(s):
        c = s[i]
        if c != "\\":
            out.extend(c.encode("utf-8"))
            i += 1
            continue
        if i + 1 >= len(s):
            out.append(ord("\\"))
            break
        n = s[i + 1]
        simple = {"n": 10, "t": 9, "r": 13, "0": 0, "\\": 92}
        if n in simple:
            out.append(simple[n])
            i += 2
        elif n == "x" and i + 3 < len(s):
            try:
                out.append(int(s[i + 2 : i + 4], 16))
                i += 4
            except ValueError:
                out.append(ord("\\"))
                i += 1
        else:
            out.append(ord("\\"))
            i += 1
    return bytes(out)


def read_cases(name: str) -> list[tuple[str, bytes, list[str]]]:
    path = CASES_DIR / f"{name}.txt"
    text = path.read_text(encoding="utf-8")
    cases: list[tuple[str, bytes, list[str]]] = []
    for n, raw in enumerate(text.splitlines(), start=1):
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        parts = line.split("|")
        if len(parts) < 2:
            raise ValueError(f"{path}:{n}: expected `label | stdin | args...`")
        label = parts[0].strip()
        stdin = unescape(parts[1].strip())
        # `shlex.split` and NOT `str.split`: a case like `-d ' '` is three
        # tokens to the latter, and the run then differs from GNU for a
        # reason that is entirely this file's fault. That happened on the
        # first `cut` run and produced three false differences, which is
        # the direction that gets acted on.
        args = shlex.split(parts[2]) if len(parts) > 2 else []
        cases.append((label, stdin, args))
    if not cases:
        raise ValueError(f"{path}: no cases — an empty list reports perfect agreement")
    return cases


def build(name: str, coreutils: bool, dest: Path) -> Path:
    """Build one side and copy the binary aside before the other overwrites it."""
    cmd = ["cargo", "build", "--target", TARGET]
    cmd += ["-p", "coreutils", "--bin", name] if coreutils else ["-p", name]
    r = subprocess.run(cmd, cwd=ROOT, capture_output=True, text=True, check=False)
    if r.returncode != 0:
        raise RuntimeError(f"build failed for {'coreutils' if coreutils else name}:\n{r.stderr[-2000:]}")
    built = ROOT / "target" / TARGET / "debug" / f"{name}.exe"
    if not built.is_file():
        raise RuntimeError(f"no binary at {built} after building")
    shutil.copy2(built, dest)
    return dest


# Per-case wall clock, and an output cap.
#
# BOTH are load-bearing and neither was there on the first run, which hung.
# "Runs forever" and "emits without end" are exactly the behaviours a
# differential is looking for -- `seq 1 inf` and `seq 1 0 5` are the obvious
# ways to ask for them -- so a harness that cannot survive them cannot report
# them. A timeout is recorded as its own outcome rather than as a crash, so a
# side that hangs where the other answers is a visible difference.
CASE_TIMEOUT_S = 5
CASE_OUTPUT_CAP = 1 << 20

TIMED_OUT = b"<<TIMED OUT>>"


def _capped(out: bytes) -> bytes:
    return out if len(out) <= CASE_OUTPUT_CAP else out[:CASE_OUTPUT_CAP] + b"<<TRUNCATED>>"


def run_local(exe: Path, args: list[str], stdin: bytes) -> tuple[bytes, bytes, int]:
    try:
        r = subprocess.run(
            [str(exe), *args],
            input=stdin,
            capture_output=True,
            check=False,
            timeout=CASE_TIMEOUT_S,
        )
    except subprocess.TimeoutExpired:
        return TIMED_OUT, b"", -1
    return _capped(r.stdout), _capped(r.stderr), r.returncode


def run_gnu(name: str, args: list[str], stdin: bytes) -> tuple[bytes, bytes, int]:
    """The host's own implementation, through WSL."""
    try:
        r = subprocess.run(
            ["wsl", "-e", name, *args],
            input=stdin,
            capture_output=True,
            check=False,
            timeout=CASE_TIMEOUT_S,
        )
    except subprocess.TimeoutExpired:
        return TIMED_OUT, b"", -1
    return _capped(r.stdout), _capped(r.stderr), r.returncode


def selftest() -> int:
    failures: list[str] = []
    checked = 0

    for src, want, label in [
        (r"a\nb", b"a\nb", "newline"),
        (r"a\tb", b"a\tb", "tab"),
        (r"a\rb", b"a\rb", "carriage return"),
        (r"\x80", b"\x80", "a byte that is not UTF-8"),
        (r"\x00", b"\x00", "NUL"),
        (r"a\\b", b"a\\b", "an escaped backslash"),
        ("plain", b"plain", "no escapes"),
        (r"\q", b"\\q", "an unknown escape stays literal"),
    ]:
        checked += 1
        got = unescape(src)
        if got != want:
            failures.append(f"{label}: unescape({src!r}) = {got!r}, wanted {want!r}")

    for spec, want, label in [
        ("-d ' ' -f 2", ["-d", " ", "-f", "2"], "a quoted space argument"),
        ("-d '' -f 1", ["-d", "", "-f", "1"], "a quoted empty argument"),
        ("-f 1,3", ["-f", "1,3"], "an ordinary argument list"),
        ("", [], "no arguments"),
    ]:
        checked += 1
        got = shlex.split(spec)
        if got != want:
            failures.append(f"{label}: shlex.split({spec!r}) = {got!r}, wanted {want!r}")

    # A case file that exists but says nothing must be an error: an empty run
    # reports perfect agreement, which is the most dangerous wrong answer this
    # tool could give.
    import tempfile

    checked += 1
    with tempfile.TemporaryDirectory() as td:
        global CASES_DIR
        saved, CASES_DIR = CASES_DIR, Path(td)
        (CASES_DIR / "empty.txt").write_text("# only a comment\n", encoding="utf-8", newline="")
        try:
            read_cases("empty")
            failures.append("an empty case list must raise, not report agreement")
        except ValueError:
            pass
        finally:
            CASES_DIR = saved

    for f in failures:
        print(f"selftest FAIL {f}")
    print(f"selftest: {checked - len(failures)}/{checked} cases pass")
    return 1 if failures else 0


def main() -> int:
    for s in (sys.stdout, sys.stderr):
        try:
            s.reconfigure(encoding="utf-8", errors="replace")
        except (AttributeError, ValueError):
            pass

    args = sys.argv[1:]
    if selftestflag.wants_selftest(args):
        return selftest()
    if not args:
        print("usage: dup-differential.py <name>", file=sys.stderr)
        return 2
    name = args[0]

    try:
        cases = read_cases(name)
    except (OSError, ValueError) as e:
        print(f"dup-differential: {e}", file=sys.stderr)
        return 2

    work = ROOT / "build" / "dup-differential"
    work.mkdir(parents=True, exist_ok=True)
    try:
        sa = build(name, coreutils=False, dest=work / f"{name}-standalone.exe")
        cu = build(name, coreutils=True, dest=work / f"{name}-coreutils.exe")
    except RuntimeError as e:
        print(f"dup-differential: {e}", file=sys.stderr)
        return 2

    sa_ok = cu_ok = 0
    sa_bad: list[str] = []
    cu_bad: list[str] = []
    for label, stdin, argv in cases:
        gnu = run_gnu(name, argv, stdin)
        if gnu == run_local(sa, argv, stdin):
            sa_ok += 1
        else:
            sa_bad.append(label)
        if gnu == run_local(cu, argv, stdin):
            cu_ok += 1
        else:
            cu_bad.append(label)

    total = len(cases)
    print(f"{name}: GNU agreement over {total} cases (stdout, stderr and exit together)")
    print(f"  standalone  {sa_ok}/{total}")
    print(f"  coreutils   {cu_ok}/{total}")
    if sa_bad:
        print(f"\n  standalone differs: {', '.join(sa_bad)}")
    if cu_bad:
        print(f"  coreutils  differs: {', '.join(cu_bad)}")
    if not sa_bad and not cu_bad:
        print("\n  Both agree with GNU everywhere asked. That is not a verdict —")
        print("  it means these cases do not separate them, so either the cases")
        print("  are too few or the choice has to be made on other grounds.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
