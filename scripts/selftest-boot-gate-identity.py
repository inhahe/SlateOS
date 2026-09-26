"""Self-test for boot-test.sh's `check_identity_rungs` gate.

The gate fails a boot in which any file-identity rung printed SKIPPED, or in
which fewer rungs reached a verdict than the kernel source defines. A rung
returns Ok(()) when it skips -- deliberately, since a kernel that genuinely
cannot hard-link should not fail a boot -- so `check_selftest_failures`, which
greps for "self-test failed", stays silent and the run is green having tested
nothing.

WHY A SELF-TEST OF A GATE. The gate is a bash function inside boot-test.sh, so
nothing else in the tree notices if an edit stops it firing, and a silent gate
plus a no-op rung produce exactly the green boot it exists to prevent. This one
has already been wrong twice:

  1. The driver was passed to bash as an argv element. Windows has no argv
     array, so the multi-line script was flattened into a command line and
     re-parsed into something that always returned 0. It scored 4 of 7 -- and
     the four that 'passed' were the four expecting failure. A harness that
     never ran the gate, reporting as a working gate.
  2. The expected rung count was the literal 4 in the gate, while the gate's
     own comment claimed a fifth rung would be 'covered by construction'. A
     fifth landed in acl.rs the same hour. The count is now derived from the
     source, and this suite drives that derivation with a synthetic tree.

Usage:  python scripts/selftest-boot-gate-identity.py
Exit:   0 all cases behave as specified, 1 otherwise.
"""

import io
import os
import shutil
import subprocess
import sys
import tempfile

NL = chr(10)
HERE = os.path.dirname(os.path.abspath(__file__))
BOOT = os.path.join(HERE, "boot-test.sh")
FUNC = "check_identity_rungs"


def _find_bash():
    """The bash `boot-test.sh` runs under, not whatever Windows picks first.

    `subprocess.run(["bash", ...])` does NOT honour PATH on Windows:
    `CreateProcess` searches System32 before PATH, so a bare "bash" resolves to
    \\`C:\\\\Windows\\\\System32\\\\bash.exe`\\ -- the WSL launcher -- even though
    `shutil.which("bash")` reports Git bash.  This suite then failed whenever
    WSL was unwell, printing `Catastrophic failure / Bash/Service/E_UNEXPECTED`
    as 4 of 10 wrong cases, which looks exactly like the gate having broken.

    The gate under test is a shell function inside a script MSYS bash runs, so
    WSL is the wrong interpreter whatever its health.
    """
    for cand in (shutil.which("bash"), "/usr/bin/bash"):
        if cand and os.path.exists(cand) and "System32" not in cand:
            return cand
    return "bash"


BASH = _find_bash()

OK_MARK = "identity rung OK"
SKIP_MARK = "identity rung SKIPPED"
OKL = "[vfs]   " + OK_MARK + " -- flock keys on identity, not name"
SKP = "[vfs]   " + SKIP_MARK + " -- /tmp does not support link()"

# (name, serial lines or None for an absent log, marker sites in source, expect_fail)
CASES = [
    ("all five ran", [OKL] * 5, 5, 0),
    ("one skipped", [OKL] * 4 + [SKP], 5, 1),
    ("all five skipped", [SKP] * 5, 5, 1),
    ("four of five ran", [OKL] * 4, 5, 1),
    ("silent: nothing", ["BOOT_OK"], 5, 1),
    ("extra kshell run", [OKL] * 6, 5, 0),
    ("source grew to 6", [OKL] * 5, 6, 1),
    ("source shrank to 2", [OKL] * 5, 2, 0),
    ("markers gone from source", [OKL] * 5, 0, 1),
    ("absent serial log", None, 5, 0),
]


def main():
    src = io.open(BOOT, encoding="utf-8").read()
    marker = FUNC + "() {"
    if marker not in src:
        print("FAIL: %s not found in boot-test.sh -- the gate was removed or renamed" % FUNC)
        return 1
    start = src.index(marker)
    func = src[start:src.index(NL + "}" + NL, start) + 3]

    root = tempfile.mkdtemp(prefix="idgate")
    fails = 0
    try:
        ksrc = os.path.join(root, "kernel", "src")
        os.makedirs(ksrc)
        # A driver FILE, run with cwd=root and bare names. An absolute path
        # here is `E:\visual studio projects\...` -- backslashes and two
        # spaces -- which MSYS bash cannot launch; that was failure (1) above.
        drv = os.path.join(root, "drv.sh")
        # PROJECT_ROOT is set INSIDE the driver, not passed in the
        # environment: an `env=` dict handed to subprocess did not survive the
        # MSYS bash shim (the variable arrived empty, the gate globbed
        # /kernel/src, and grep counted its own error line as one match, so
        # every case took the markers-gone branch). A line in the script is
        # not subject to env propagation.
        io.open(drv, "w", encoding="utf-8", newline=NL).write(
            'PROJECT_ROOT="."' + NL + func + NL + FUNC + ' "$1"' + NL)

        for name, serial, sites, want in CASES:
            # The gate counts marker sites in $PROJECT_ROOT/kernel/src, so the
            # expectation is driven by a synthetic tree rather than by the real
            # one -- otherwise every case would move whenever a rung is added.
            io.open(os.path.join(ksrc, "fake.rs"), "w", encoding="utf-8",
                    newline=NL).write(NL.join(['let _ = "' + OK_MARK + '";'] * sites) + NL)
            log = os.path.join(root, "serial.log")
            if serial is None:
                if os.path.exists(log):
                    os.unlink(log)
            else:
                io.open(log, "w", encoding="utf-8", newline=NL).write(NL.join(serial) + NL)

            r = subprocess.run([BASH, "drv.sh", "serial.log"], capture_output=True,
                               text=True, cwd=root)
            # Separate a gate VERDICT from a harness that never ran the gate.
            # Without this the suite is unfalsifiable in one direction: if bash
            # cannot launch the driver, every case returns non-zero and every
            # failure-expecting case reports ok. bash exits 127 in that event.
            if r.returncode > 125 or "No such file" in r.stderr:
                print("  HARNESS FAILED to run the gate: rc=%d %s"
                      % (r.returncode, r.stderr.strip()[:70]))
                return 1
            got = 0 if r.returncode == 0 else 1
            if got != want:
                fails += 1
            print("  %s%-26s sites=%d expected_fail=%d got=%d"
                  % ("ok  " if got == want else "BAD ", name, sites, want, got))
    finally:
        shutil.rmtree(root, ignore_errors=True)

    if fails:
        print("FAIL: %d of %d case(s) wrong" % (fails, len(CASES)))
        return 1
    print("ok: all %d case(s) behave as specified" % len(CASES))
    return 0


if __name__ == "__main__":
    sys.exit(main())
