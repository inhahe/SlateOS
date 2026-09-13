#!/usr/bin/env python3
"""Find a program that reports a failure on stderr and then exits 0.

THE RULE, which is not new here. `scripts/check-usage-status.py` already
guards it: *if a command tells the user it could not do what it was asked, it
must set a non-zero exit status*. That checker is **static** and scoped to
**kshell**. This one is **behavioural** and scoped to **userspace binaries**,
which is a different population reached by a different instrument.

WHY BOTH ARE NEEDED. Three instances turned up in one night's work on an
unrelated class, none of them a kshell command and none visible to a source
grep:

    m4 --zzq               "m4: unknown option: --zzq"        exit 0
    ftp --zzq              "ftp: unknown option: --zzq"       exit 0
    coredump-extract --zzq "coredumpctl: unknown option..."   exit 0

The first two printed the diagnostic and carried on. The third was worse: a
caller dropped the status of a function that had just started returning one,
so the guard fired, printed, and the personality exited 0 anyway. Rust does
not warn on a discarded `i32`; the build was clean and the tests passed.

**This is the most deceptive shape in the family.** A human reading the
terminal is told the truth. A script reading `$?` is told the run succeeded.
Only the one nobody watches is believed -- and a grep for the message marks
the program as *already fixed*, because the message is right.

THE PROBE. Each binary is run in an empty directory with a path that does not
exist. A program that takes a file should fail; a program that takes no
operand should refuse the operand. Either way the correct status is non-zero.
Flagged only when stderr looks like a *failure* rather than a warning -- see
`SOUNDS_LIKE_FAILURE` -- so an advisory note on stderr is not a finding.

WHAT A CLEAN RUN DOES NOT MEAN. A program that fails silently is invisible
here: no stderr, nothing to match. This finds programs that *say* something
and then contradict it, which is a narrower claim than "every exit status in
the tree is right".
"""

import argparse
import glob
import os
import re
import shutil
import subprocess
import sys
import tempfile

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import selftestflag  # noqa: E402  (needs the path line above)

PER_BINARY_TIMEOUT = 5

# The operand handed to every program: a path that cannot exist.
MISSING_PATH = "zzq-no-such-file-8f3a"

# Words that make a stderr line a report of failure rather than a warning or a
# progress note. Matched case-insensitively on whole words where the word form
# matters, so "unrecognized" and "unrecognised" both count and "notice" does
# not match "not".
SOUNDS_LIKE_FAILURE = re.compile(
    r"\b("
    r"error|errors"
    r"|cannot|can't|unable"
    r"|unknown|unrecognized|unrecognised|invalid|illegal|bad"
    r"|failed|failure"
    r"|no such|not found|does not exist|missing"
    r"|refus\w+|denied|not permitted"
    r"|usage"
    r")\b",
    re.IGNORECASE,
)

# Programs for which exit 0 with a diagnostic is correct.
#
# `true` must exit 0 whatever happens. `echo`/`printf` treat every argument as
# an operand and print it. `yes` loops until it is killed, so its status is
# never observed. `test`/`[` report through the status only and print nothing.
EXPECTED_ZERO = {"true", "echo", "printf", "yes", "test", "[", "false"}

# Programs whose exit 0 with a diagnostic is *verified correct*, with the
# evidence. Not a baseline to grow: each line is a measurement, and an entry
# without one does not belong here.
#
# `ed` -- POSIX: naming a file that does not exist opens a new buffer, and
#   quitting without a further error exits 0. Not reasoned from the standard
#   but measured: `scripts/ed-diff.sh` runs 507 cases against GNU ed 1.20.1
#   and compares stdout, stderr, the exit status and the bytes on disk.
# `hwinfo` -- "cannot read /proc/cpuinfo (...); CPU details unavailable" on a
#   host with no /proc. The inventory is still produced and still honest: the
#   source argues, correctly, that a machine running hwinfo *has* a processor,
#   so "details unknown" is a true statement and the line exists to say why.
#   Exempted by name because the wording rule that would catch it also
#   suppresses real failures -- see ANNOUNCES_A_FALLBACK.
# `ftp` -- the probe hands every program a *filename*, and ftp's operand is a
#   *hostname*. So this measures what ftp does with a host that does not
#   resolve, which is an ordinary interactive situation: BSD ftp reports it
#   and drops to the `ftp>` prompt, and with stdin at EOF it exits. Whether
#   that status is 0 upstream I could not establish -- the reference ftp on
#   this host hung on the lookup rather than answering -- so this is exempted
#   as a *probe mismatch*, which is a claim about my instrument and not about
#   ftp. If someone wants ftp's startup-failure status settled, the probe for
#   it is a bad host, not a bad path.
VERIFIED_ZERO = {
    "ed": "507-case differential against GNU ed 1.20.1 (scripts/ed-diff.sh)",
    "hwinfo": "advisory about one section of an inventory that is still produced",
    "ftp": "probe mismatch: the operand is a hostname, not a file",
}

# Daemons and interactive programs: launching them is either useless or
# harmful, and the unknown-option sweep skips them for the same reason. Kept
# as a literal list rather than a heuristic so adding one is a deliberate act.
SKIP = {
    "init", "getty", "login", "sshd", "ftpd", "inetd", "dbus", "servicebus",
    "logind", "udevd", "crond", "ntpd", "dhcpcd", "thermald", "fwupd",
    "irqbalance", "tuned", "loginmgr", "sysmon", "atd",
}


def probe(argv, cwd):
    """Run `argv`; return (exit_code, stderr_text) or (None, "") if it could
    not be launched or had to be killed."""
    with open(os.devnull, "rb") as devnull:
        try:
            proc = subprocess.run(
                argv,
                cwd=cwd,
                stdin=devnull,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.PIPE,
                timeout=PER_BINARY_TIMEOUT,
            )
        except (subprocess.TimeoutExpired, OSError):
            return (None, "")
    return (proc.returncode, proc.stderr.decode("utf-8", "replace"))


# A line that says what it is doing *instead* is a warning, not a report of
# failure, even though it usually contains a failure word. Two real examples
# from the first run, both correct as written and both flagged by the words
# alone:
#
#   hwinfo:  "cannot read /proc/cpuinfo (...); CPU details unavailable"
#   selinux: "unknown personality 'selinux', defaulting to getenforce"
#
# The first is an advisory about one section of an inventory that is otherwise
# produced honestly -- the source comment argues, correctly, that a machine
# running hwinfo *has* a processor, so "details unknown" is a true statement
# and the line exists to say *why*. The second announces a documented fallback
# and then does the work.
#
# Narrow on purpose, and narrower than my first attempt. That version also
# matched `unavailable` and `not available`, which made it suppress
#
#   credentials: "Failed to set master password: the system random number
#                 generator is unavailable"
#
# -- a real failure report, because those words usually name the *reason* a
# thing failed rather than a substitute that was used instead. Trading a false
# positive for a false negative is the worse of the two swaps: a noisy
# detector is argued with, a quiet one is believed. So the rule requires an
# explicit substitution, and `hwinfo` is exempted by name below rather than by
# widening this.
ANNOUNCES_A_FALLBACK = re.compile(
    r"\b(defaulting to|falling back|using instead)\b",
    re.IGNORECASE,
)


def first_failure_line(text):
    """The first stderr line that reads as a report of failure, or None."""
    for line in text.splitlines():
        line = line.strip()
        if not line or not SOUNDS_LIKE_FAILURE.search(line):
            continue
        if ANNOUNCES_A_FALLBACK.search(line):
            continue
        return line
    return None


def selftest():
    """Prove the detector fires on the defect and stays quiet without it.

    Four synthetic programs, because a detector that only ever says yes is as
    useless as one that only ever says no -- and the two quiet cases are the
    ones that would make this unusable if they were wrong.
    """
    py = sys.executable
    cases = [
        ("says-and-exits-0", 'import sys; sys.stderr.write("x: cannot open z\\n")', 0, True),
        ("says-and-exits-1", 'import sys; sys.stderr.write("x: cannot open z\\n"); sys.exit(1)', 1, False),
        ("silent-exits-0", "pass", 0, False),
        ("warns-and-exits-0", 'import sys; sys.stderr.write("x: using defaults\\n")', 0, False),
    ]
    bad = 0
    work = tempfile.mkdtemp(prefix="sezsweep-selftest-")
    try:
        for name, body, _want_code, want_flag in cases:
            script = os.path.join(work, name + ".py")
            with open(script, "w", encoding="utf-8") as fh:
                fh.write(body + "\n")
            code, err = probe([py, script, MISSING_PATH], work)
            flagged = code == 0 and first_failure_line(err) is not None
            if flagged != want_flag:
                print("  selftest FAIL (%s): wanted flagged=%s got %s"
                      % (name, want_flag, flagged))
                bad += 1
    finally:
        shutil.rmtree(work, ignore_errors=True)

    # And that the failure-word test is about words, not substrings.
    for text, want in (("notice: ok", False), ("cannot open", True),
                       ("bad input", True), ("embadded", False),
                       # A fallback announcement is a warning, not a failure.
                       ("unknown personality 'x', defaulting to getenforce", False),
                       # ...but continuing is not substituting, and a resource
                       # that is unavailable is usually why something failed.
                       ("cannot open x: failed, continuing", True),
                       ("Failed to set master password: the RNG is unavailable", True)):
        got = first_failure_line(text) is not None
        if got != want:
            print("  selftest FAIL (wording %r): wanted %s got %s" % (text, want, got))
            bad += 1

    print("stderr-exit-zero-sweep: selftest %s (%d cases)"
          % ("FAILED" if bad else "ok", len(cases) + 4))
    return 1 if bad else 0


def main(argv=None):
    ap = argparse.ArgumentParser(allow_abbrev=False)
    ap.add_argument("--dir", default="target/x86_64-pc-windows-gnu/debug")
    ap.add_argument("--self-test", "--selftest", "--self_test",
                    dest="selftest", action="store_true")
    args = ap.parse_args(argv)

    if args.selftest or selftestflag.wants_selftest(sys.argv[1:]):
        return selftest()

    exes = sorted(glob.glob(os.path.join(args.dir, "*.exe")))
    if not exes:
        print("stderr-exit-zero-sweep: no binaries under %s -- nothing was "
              "examined, which is not the same as nothing being wrong."
              % args.dir)
        return 2

    findings = []
    skipped = 0
    unlaunchable = 0
    work = tempfile.mkdtemp(prefix="sezsweep-")
    try:
        for exe in exes:
            name = os.path.basename(exe)[: -len(".exe")]
            if name in SKIP or name in EXPECTED_ZERO or name in VERIFIED_ZERO:
                skipped += 1
                continue
            code, err = probe([exe, MISSING_PATH], work)
            if code is None:
                unlaunchable += 1
                continue
            if code != 0:
                continue
            line = first_failure_line(err)
            if line:
                findings.append((name, line))
    finally:
        shutil.rmtree(work, ignore_errors=True)

    print("stderr-exit-zero-sweep: probed with a path that cannot exist")
    print("  binaries on disk:   %d" % len(exes))
    print("  skipped:            %d (incl. %d verified-correct)"
          % (skipped, len(VERIFIED_ZERO)))
    print("  did not launch:     %d" % unlaunchable)
    print("  REPORTED AND EXITED 0: %d" % len(findings))
    if findings:
        print()
        print("  said it failed, then told the shell it had not:")
        for name, line in findings:
            print("    %-22s %s" % (name, line[:70]))
        print()
        print("A human reading the terminal is told the truth here; a script")
        print("reading $? is told the run succeeded. Fix the status, not the")
        print("message -- the message is already right, which is why a grep")
        print("for it marks these as already fixed.")
        return 1

    print()
    print("Clean does NOT mean every exit status in the tree is right: a")
    print("program that fails silently says nothing and is invisible here.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
