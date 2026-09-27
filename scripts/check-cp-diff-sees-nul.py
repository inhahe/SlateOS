#!/usr/bin/env python3
"""Prove `cp-diff.sh`'s `contents()` can still see a NUL-only difference.

WHY THIS EXISTS
===============

`cp-diff.sh` captures each tree's file contents in a command substitution::

    o_body=$(contents "$o_dir" | scrub "$o_dir")

A command substitution **drops NUL bytes** — bash even says so, once per file::

    warning: command substitution: ignored null byte in input

Both sides lose them identically, so two files differing *only* in NUL bytes
compared EQUAL and the harness reported the copy as faithful. That is comparing
a lossy projection of the thing rather than the thing.

The fix (`49416d81a`, 2026-09-14) prints a `sha` line per file above the body.
The hash is plain hex, so it survives the capture; the readable body stays
beneath it so a failure is still diagnosable rather than a wall of hashes.

`TD-B-CP-DIFF-CANNOT-SEE-A-DIFFERENCE-MADE-OF-NUL-BYTES` asked for two things
before that change could be trusted, and said plainly why: *"A harness change
that has not been shown to (a) catch the case it is for and (b) still call
identical trees identical is exactly the kind that turns green into noise for
the other two lanes."* The change was committed without either. This is both,
made permanent.

WHY IT EXTRACTS THE FUNCTION RATHER THAN REIMPLEMENTING IT
===========================================================

It pulls the real `contents()` text out of `cp-diff.sh` and sources that. A
reimplementation would test this file's idea of the function, which is the
mirror problem — the corpus and the expectation coming from one place. If
someone edits `contents()`, this runs the edited version.

It does NOT invoke `cp-diff.sh` itself: that harness runs ~90 differential
cases against a built GNU reference and takes minutes. The unit under test is
one function.

WHY NOT A `--self-test` FLAG IN `cp-diff.sh`
=============================================

`diff-wsl.sh` re-execs the harness inside WSL and forwards its arguments, so a
new top-level flag is a change to a script all three lanes depend on, for a
check that needs none of its machinery. The entry above is about exactly this
kind of risk. A separate file touches nothing.

WHICH BASH, AND WHICH TOOLS
===========================

The probe runs under `proctree.find_unix_shell()` -- Git's bash here, the
system bash on Linux -- and puts that shell's own `/usr/bin` first on `PATH`.

Until 2026-09-25 it ran `subprocess.run(['bash', ...])`, and on Windows that
is **WSL's** bash: `CreateProcess` searches `System32` before `PATH`, and
`C:\\Windows\\System32\\bash.exe` is the WSL launcher (`known-issues.md` ->
"`subprocess.run(["bash", ...])` gets WSL's bash, not Git's"). So every run of
this gate started a Linux VM, and when the VM did not start in time -- WSL
answered `HCS_E_CONNECTION_TIMEOUT` under a loaded lane-E boot test -- the
self-test said the gate "fails its own cases" and the boot test refused to
build a tree nothing was wrong with. The comments below that blamed MSYS for
two faults were written from inside that VM; both faults are WSL's, and the
2026-09-25 notes beside them say what was measured.

What the probe grades does not depend on which of the two it is: whether the
function's per-file hash line survives a `$(...)` capture. Both run bash 5 with
GNU `find`, `sort`, `sha256sum` and `cut`. But only one of them is always
there, and it is the one `boot-test.sh` itself runs under.

`/usr/bin` goes first because a bash started from a native Windows parent
inherits that parent's `PATH`, and from PowerShell that finds
`C:\\Windows\\System32\\find.exe` and `sort.exe` -- a string search and a line
sorter that take none of these options -- and no `sha256sum` at all. Measured:
Git's bash with `PATH=C:\\Windows\\System32` answers `command -v find` with
`/c/Windows/System32/find`. The shell's own tools are the ones it ships with.
"""

import contextlib
import io
import re
import shutil
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import proctree  # noqa: E402  (needs the path above)

ROOT = Path(__file__).resolve().parent.parent
HARNESS = ROOT / "scripts" / "cp-diff.sh"

# Exit codes, as `scripts/run-checker.sh` reads them: 0 clean, 1 a finding,
# 2 no verdict (the build stops), 3 "I could not run, and here is why" (the
# gate is listed as skipped, never as passed). A skip here used to return 0,
# which is the behaviour exit 3 was defined to replace.
NO_VERDICT = 2
SKIPPED = 3


@contextlib.contextmanager
def scratch():
    """A scratch tree UNDER THE REPO, removed afterwards.

    Not `tempfile.TemporaryDirectory()`: that yields `C:/Users/...`, which
    the bash this ran under could not resolve, and the failure was quiet
    enough to have produced a confident wrong verdict once already. `build/`
    is gitignored. (That bash was WSL's, not MSYS's -- see WHICH BASH above.
    Git's bash opens `C:/Users/...` fine; the repo-relative path is kept
    because it works under every bash, which a drive-letter path does not.)
    """
    tmp = ROOT / "build" / "cp-diff-nul-probe"
    shutil.rmtree(tmp, ignore_errors=True)
    tmp.mkdir(parents=True, exist_ok=True)
    try:
        yield tmp
    finally:
        shutil.rmtree(tmp, ignore_errors=True)

# The function, from `contents() {` to the `}` at column zero.
FUNC_RE = re.compile(r"^contents\(\) \{.*?^\}", re.M | re.S)


def extract_contents(text):
    """The real `contents()` source, or None."""
    m = FUNC_RE.search(text)
    return m.group(0) if m else None


def run_probe(func_src, tmp, bash):
    """Return the probe's (stdout, stderr) for three trees, run under `bash`.

    `a` and `c` are byte-identical. `b` differs from `a` ONLY in how many NUL
    bytes its single file holds — same length otherwise is not required, only
    that no non-NUL byte differs, because that is the case the capture used to
    erase.

    `bash` is an absolute path from `proctree.find_unix_shell()`, never the
    bare word: see WHICH BASH at the top of this file.

    # The scratch tree lives under the repo, not in `%TEMP%`

    The bash this ran under could not resolve a `C:/Users/...` path (it was
    WSL's; the note below said MSYS). The first version of this
    gate used `tempfile.TemporaryDirectory()`, so `. '<winpath>/fn.sh'` failed
    with "No such file or directory", `contents` was never defined, both
    captures came back EMPTY — and empty equals empty, so the gate reported
    "two trees differing only in NUL bytes compare EQUAL" and told the reader
    `contents()` needed its hash line back. The harness was fine. The gate
    could not run and said the subject was broken.

    That is the defect this whole family of entries is about, produced by the
    checker written to catch one instance of it. The path fix is half the
    repair; `ran_at_all` below is the other and the more important one,
    because the next thing that stops this running will not be a path.
    """
    for name, body in (("a", b"x\0y"), ("b", b"x\0\0y"), ("c", b"x\0y")):
        d = tmp / name
        d.mkdir(parents=True, exist_ok=True)
        (d / "f.bin").write_bytes(body)

    fn = tmp / "contents_fn.sh"
    # A marker the probe prints only if the function is actually defined, so
    # "could not run" and "ran and found nothing" stop being the same output.
    # newline='' so Python does NOT translate to CRLF. A shell script with
    # carriage returns dies as a syntax error near an unexpected token, the
    # function is never defined, the probe produces nothing, and nothing
    # compares equal to nothing. That was the SECOND fault hiding behind the
    # first here -- two independent ways to not run, both silent, both
    # reading as a verdict. It is why ran_at_all() is not optional.
    with io.open(fn, "w", encoding="utf-8", newline="") as fh:
        fh.write(func_src + chr(10))

    # Through a command substitution, which is the whole point: the capture is
    # what ate the NULs, so a probe that read the function's output on a pipe
    # would not reproduce the defect at all.
    # Relative to the repo root, which `cwd=` below makes the working
    # directory: MSYS bash resolves these and does not resolve `C:/...`.
    rel = tmp.relative_to(ROOT).as_posix()

    # A SCRIPT FILE, not `bash -c`.
    #
    # Under the bash this ran under, a function defined by `bash -c` was NOT
    # visible inside a command substitution: `A=$(contents ...)` failed with
    # `contents: command not found` while `declare -F contents` in the same
    # script reported it defined. Run from a file, the function survives.
    #
    # This said MSYS, and blamed its fork emulation. Measured 2026-09-25 with
    # `f() { echo hi; }; declare -F f && A=$(f)` as a `-c` string: WSL's
    # launcher prints DEFINED, then `/bin/bash: line 1: f: command not found`;
    # Git's bash prints DEFINED and `A=[hi]`. It was WSL. The script file stays,
    # because it is right under both.
    #
    # The command substitution cannot be dropped to work around it: the
    # capture is the thing that eats NUL bytes, so a probe reading the
    # function on a pipe would not reproduce the defect at all.
    #
    # `NOSHA` before anything else: without `sha256sum` the real function
    # prints an empty hash for every file, catches nothing, and would be
    # reported as broken. `cp-diff.sh` declares `DIFF_NEED=sha256sum` and
    # skips without it, so there is nothing here to grade either.
    probe = tmp / 'probe.sh'
    script = (
        'PATH=/usr/bin:$PATH\n'
        'command -v sha256sum >/dev/null 2>&1 || {{ echo NOSHA; exit 4; }}\n'
        '. ./{rel}/contents_fn.sh || exit 3\n'
        'declare -F contents >/dev/null || exit 3\n'
        'echo DEFINED\n'
        'A=$(contents ./{rel}/a)\n'
        'B=$(contents ./{rel}/b)\n'
        'C=$(contents ./{rel}/c)\n'
        '[ -n "$A" ] && echo NONEMPTY || echo EMPTY\n'
        '[ "$A" = "$B" ] && echo AB_SAME || echo AB_DIFF\n'
        '[ "$A" = "$C" ] && echo AC_SAME || echo AC_DIFF\n'
    ).format(rel=rel)
    with io.open(probe, 'w', encoding='utf-8', newline='') as fh:
        fh.write(script)

    try:
        out = subprocess.run(
            [bash, './' + rel + '/probe.sh'],
            capture_output=True,
            text=True,
            timeout=120,
            cwd=str(ROOT),
        )
    except subprocess.TimeoutExpired:
        # No markers, so the caller reports "could not run" -- which is what
        # this is, and not a finding.
        return "", "no answer in 120 s"
    return out.stdout, out.stderr


def no_sha(stdout):
    """Did the probe find no `sha256sum` in its shell?"""
    return "NOSHA" in stdout


def tail(text, lines=6):
    """The last few lines of a probe's stderr, for a cannot-run report."""
    return "\n".join(text.strip().splitlines()[-lines:]) or "(nothing on stderr)"


def ran_at_all(stdout):
    """Did the probe actually execute `contents()`?

    Two markers, because one is not enough. `DEFINED` says the sourcing worked
    and the function exists; `NONEMPTY` says calling it produced output. A
    function that is defined and returns nothing would otherwise compare equal
    to itself on every tree and read as a clean verdict — which is precisely
    how the first version of this gate reported a working harness as broken.

    A comparison that could not be made is not a comparison that failed.
    """
    return "DEFINED" in stdout and "NONEMPTY" in stdout


def verdict(stdout):
    """(catches_nul, identical_still_equal) from the probe's markers."""
    return ("AB_DIFF" in stdout, "AC_SAME" in stdout)


def selftest():
    """The checker must FAIL on a `contents()` with the hash removed.

    Without this, a checker that always printed OK would pass its own gate and
    report a harness as sound forever. The sabotage is the exact regression it
    exists to catch: delete the `sha` line and the NUL difference becomes
    invisible again.
    """
    text = HARNESS.read_text(encoding="utf-8", errors="replace")
    real = extract_contents(text)
    bad = 0

    if real is None:
        print("FAIL could not extract contents() from cp-diff.sh")
        return 1

    bash = proctree.find_unix_shell()
    if bash is None:
        print(
            "SKIPPED no Unix shell to run contents() under (Git Bash, MSYS2, "
            "or bash on a Unix host; SLATE_BASH names one elsewhere)"
        )
        return SKIPPED

    with scratch() as tmp:
        out, err = run_probe(real, tmp / "real", bash)
        if no_sha(out):
            print(
                "SKIPPED no sha256sum in %s's /usr/bin -- cp-diff.sh skips "
                "without it, so there is nothing to grade" % bash
            )
            return SKIPPED
        if not ran_at_all(out):
            # Exit 2, not 1: this is the gate failing to run, and the build
            # stops on it as "no verdict" rather than calling the tree broken.
            # It returned 1 while printing the sentence below.
            print("NO VERDICT the probe could not run contents() under %s" % bash)
            print("     stdout: %r" % out)
            print("     stderr: %s" % tail(err))
            print("     A gate that cannot run must say so, not report a finding.")
            return NO_VERDICT
        print("     (probe shell: %s)" % bash)
        catches, same = verdict(out)
        bad += 0 if catches else 1
        bad += 0 if same else 1
        print("%-4s the real contents() catches a NUL-only difference"
              % ("ok" if catches else "FAIL"))
        print("%-4s ...and still calls identical trees identical"
              % ("ok" if same else "FAIL"))

        # SABOTAGE: drop the hash line, leaving the body capture alone.
        without = "\n".join(
            l for l in real.split("\n") if "sha256sum" not in l and "printf 'sha" not in l
        )
        out_bad, err_bad = run_probe(without, tmp / "sabotaged", bash)
        if not ran_at_all(out_bad):
            print("NO VERDICT the sabotaged probe could not run: %r" % out_bad)
            print("     stderr: %s" % tail(err_bad))
            return NO_VERDICT
        catches_bad, same_bad = verdict(out_bad)
        ok = (not catches_bad) and same_bad
        bad += 0 if ok else 1
        print(
            "%-4s a contents() with no hash CANNOT see it -- so this gate can fail"
            % ("ok" if ok else "FAIL")
        )
        if not ok:
            print(
                "       sabotaged run reported catches=%r identical=%r; a gate that "
                "passes its own sabotage is measuring nothing"
                % (catches_bad, same_bad)
            )

    print()
    print("check-cp-diff-sees-nul selftest: 3 case(s), %d failed" % bad)
    return 1 if bad else 0


def main(argv=None):
    argv = sys.argv[1:] if argv is None else argv
    if "--self-test" in argv or "--selftest" in argv:
        return selftest()

    if not HARNESS.is_file():
        print("check-cp-diff-sees-nul: no scripts/cp-diff.sh here", file=sys.stderr)
        return NO_VERDICT
    # Both skips below returned 0 until 2026-09-25, which `run-checker.sh`
    # counts as a pass: a skip that reads like a pass is what this whole entry
    # is about. Exit 3 lists the gate as skipped, with this line as the reason.
    bash = proctree.find_unix_shell()
    if bash is None:
        print(
            "check-cp-diff-sees-nul: SKIPPED -- no Unix shell to run "
            "contents() under (Git Bash, MSYS2, or bash on a Unix host)"
        )
        return SKIPPED

    real = extract_contents(HARNESS.read_text(encoding="utf-8", errors="replace"))
    if real is None:
        print(
            "check-cp-diff-sees-nul: could not find `contents()` in cp-diff.sh -- "
            "it was renamed or restructured, and this gate is now grading nothing",
            file=sys.stderr,
        )
        return NO_VERDICT

    with scratch() as tmp:
        out, err = run_probe(real, tmp / "live", bash)
        if no_sha(out):
            # The harness declares DIFF_NEED=sha256sum and skips without it,
            # so this gate has nothing to grade either.
            print(
                "check-cp-diff-sees-nul: SKIPPED -- no sha256sum in %s's "
                "/usr/bin; cp-diff.sh skips without it, so there is nothing "
                "to grade" % bash
            )
            return SKIPPED
        if not ran_at_all(out):
            print(
                "check-cp-diff-sees-nul: CANNOT GRADE -- the probe never "
                "executed `contents()` under %s (markers: %r; stderr: %s). "
                "Reporting that rather than a verdict: a check that could not "
                "run is not a check that passed, and it is certainly not one "
                "that found something." % (bash, out, tail(err)),
                file=sys.stderr,
            )
            return NO_VERDICT
        catches, same = verdict(out)

    if not catches:
        print(
            "check-cp-diff-sees-nul: FAILED -- two trees differing only in NUL "
            "bytes compare EQUAL through cp-diff.sh's capture.",
            file=sys.stderr,
        )
        print(
            "    A command substitution drops NUL bytes, so the body alone cannot "
            "carry them. `contents()` needs its per-file `sha` line back.",
            file=sys.stderr,
        )
        return 1
    if not same:
        print(
            "check-cp-diff-sees-nul: FAILED -- byte-identical trees compared "
            "DIFFERENT, which would make every cp-diff run noise.",
            file=sys.stderr,
        )
        return 1

    print(
        "check-cp-diff-sees-nul: OK (a NUL-only difference is visible through "
        "the capture, and identical trees still compare equal)"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
