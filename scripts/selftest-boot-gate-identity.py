"""Self-test for boot-test.sh's `check_identity_rungs` gate.

The gate fails a boot in which any of the four file-identity rungs printed
SKIPPED, or in which fewer than four reached a verdict at all. That matters
because a skip returns SUCCESS from the rung -- deliberately, since a kernel
that genuinely cannot hard-link should not fail a boot -- so nothing else in
the harness notices. `check_selftest_failures` greps for "self-test failed",
which a skip does not print.

WHY A SELF-TEST OF A GATE. The gate exists to stop a green boot being read as
evidence for the FileId conversion when the rungs no-op'd. A gate that cannot
itself fire is exactly the same defect one level up, and this one did not fire
for its first two runs here: the driver passed the function to bash as an argv
element, and Windows has no argv array, so the multi-line script was flattened
into a command line and re-parsed into something that always returned 0. Two
of five cases passed anyway -- the two expecting success -- which is how a
broken harness reports as a working one. Run from a file it is correct.

Usage:  python scripts/selftest-boot-gate-identity.py
Exit:   0 all cases behave as specified, 1 otherwise.
"""

import io
import os
import subprocess
import sys

NL = chr(10)
HERE = os.path.dirname(os.path.abspath(__file__))
BOOT = os.path.join(HERE, "boot-test.sh")
FUNC = "check_identity_rungs"

OKL = "[vfs]   identity rung OK -- flock keys on identity, not name (hard link)"
SKP = "[vfs]   identity rung SKIPPED -- /tmp does not support link()"

# (name, fixture lines or None for an absent file, expected failure 0/1)
CASES = [
    ("all four ran", [OKL] * 4, 0),
    ("one skipped", [OKL] * 3 + [SKP], 1),
    ("all four skipped", [SKP] * 4, 1),
    ("three ran, no skip", [OKL] * 3, 1),
    ("silent: nothing", ["BOOT_OK"], 1),
    ("kshell extra run", [OKL] * 5, 0),
    ("absent file", None, 0),
]


def main():
    src = io.open(BOOT, encoding="utf-8").read()
    marker = FUNC + "() {"
    if marker not in src:
        print("FAIL: %s not found in boot-test.sh -- the gate was removed or renamed" % FUNC)
        return 1
    start = src.index(marker)
    end = src.index(NL + "}" + NL, start) + 3
    func = src[start:end]

    # A FILE, not an argv element. See the module docstring.
    drv = os.path.join(HERE, "_identity_gate_driver.sh")
    fx = os.path.join(HERE, "_identity_gate_fixture.log")
    io.open(drv, "w", encoding="utf-8", newline=NL).write(
        func + NL + FUNC + ' "$1"' + NL)

    fails = 0
    try:
        for name, fixture, want in CASES:
            if fixture is None:
                if os.path.exists(fx):
                    os.unlink(fx)
                arg = fx
            else:
                io.open(fx, "w", encoding="utf-8", newline=NL).write(NL.join(fixture) + NL)
                arg = fx
            r = subprocess.run(["bash", os.path.basename(drv), os.path.basename(arg)],
                               capture_output=True, text=True, cwd=HERE)
            # Distinguish a gate VERDICT from a harness that never ran the
            # gate. Without this the whole suite is unfalsifiable in one
            # direction: if bash cannot launch the driver, every case
            # returns non-zero and every failure-expecting case reports ok.
            if r.returncode > 125 or "No such file" in r.stderr:
                print("  HARNESS FAILED to run the gate: rc=%d %s"
                      % (r.returncode, r.stderr.strip()[:70]))
                return 1
            got = 0 if r.returncode == 0 else 1
            if got != want:
                fails += 1
            print("  %s%-19s expected=%d got=%d"
                  % ("ok  " if got == want else "BAD ", name, want, got))
    finally:
        for p in (drv, fx):
            if os.path.exists(p):
                os.unlink(p)

    if fails:
        print("FAIL: %d of %d case(s) wrong" % (fails, len(CASES)))
        return 1
    print("ok: all %d case(s) behave as specified" % len(CASES))
    return 0


if __name__ == "__main__":
    sys.exit(main())
