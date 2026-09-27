#!/usr/bin/env python3
"""Regression tests for `scripts/check-cp-diff-sees-nul.py`.

Run directly (no pytest dependency):

    python scripts/test-check-cp-diff-sees-nul.py

The pre-push hook's gate 20 pairs `test-<stem>.py` with `<stem>.py` and is
meant to run this whenever the checker is pushed. Until
`requests/e-ab-pre-push-suites-never-run-on-a-multi-commit-push.md` is fixed
it does so only for a push of one or two commits; run it by hand.

What it pins down, and why each is here:

* **The probe never runs under the bare word `bash`.** On Windows that is
  `C:\\Windows\\System32\\bash.exe`, the WSL launcher, because `CreateProcess`
  searches System32 before `PATH`. The checker ran there by accident until
  2026-09-25, and a WSL VM that did not start in time
  (`HCS_E_CONNECTION_TIMEOUT`) made a lane-E boot test refuse to build a sound
  tree: the self-test reported "fails its own cases" about a probe that had
  never run.
* **Its exit codes are the ones `scripts/run-checker.sh` reads.** 0 clean, 1 a
  finding, 2 no verdict, 3 skipped. A probe that could not run returned 1 --
  a finding -- while printing "a gate that cannot run must say so, not report
  a finding"; a skip returned 0, which the tally counts as a pass.
* **The finding is still a finding.** A `contents()` with its hash line
  removed must still exit 1, or the fixes above bought honesty about skips at
  the price of the one thing the gate is for.
"""

from __future__ import annotations

import contextlib
import importlib.util
import io
import os
import subprocess
import sys
import types
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import proctree  # noqa: E402  (needs the path above)

FAILURES: list[str] = []

# Both spellings of the checker's self-test flag, which it must treat alike
# (`scripts/check-selftest-flag-spellings.py`): every self-test case below runs
# under each, so a checker that answered only one would fail here rather than
# pass a mistyped command by running its scan.
SELF_TEST_SPELLINGS = ("--self-test", "--selftest")

# Every way of running the checker: its self-test under each spelling, and the
# gate proper.
MODES = [(f"self-test ({flag})", [flag]) for flag in SELF_TEST_SPELLINGS] + [("gate", [])]


def check(name: str, cond: bool, detail: str = "") -> None:
    if cond:
        print(f"ok   {name}")
    else:
        FAILURES.append(name)
        print(f"FAIL {name}" + (f"\n       {detail}" if detail else ""))


def load():
    """A fresh copy of the checker, so one case's substitutions cannot leak.

    Fresh as far as its own names go. `proctree` and `subprocess` are shared
    module objects, so a case must replace the checker's *reference* to them
    (`stub_shell`, `stub_run`) and never assign into them: the first draft of
    this suite set `mod.proctree.find_unix_shell = lambda: None`, which is the
    real `proctree` for everyone, and every live case after it skipped for
    want of a shell.
    """
    spec = importlib.util.spec_from_file_location(
        "check_cp_diff_sees_nul", HERE / "check-cp-diff-sees-nul.py"
    )
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


def stub_shell(mod, path):
    """Make the checker's shell lookup answer `path`, for this copy only."""
    mod.proctree = types.SimpleNamespace(find_unix_shell=lambda: path)


def stub_run(mod, fake):
    """Make the checker's `subprocess.run` be `fake`, for this copy only."""
    mod.subprocess = types.SimpleNamespace(
        run=fake, TimeoutExpired=subprocess.TimeoutExpired
    )


def run(fn, *args):
    """(exit code, first non-blank line, all output) of `fn(*args)`.

    The first line matters: `run-checker.sh` quotes it as a skip's reason, and
    refuses a skip whose reason is blank.
    """
    buf = io.StringIO()
    with contextlib.redirect_stdout(buf), contextlib.redirect_stderr(buf):
        code = fn(*args)
    text = buf.getvalue()
    first = next((line for line in text.splitlines() if line.strip()), "")
    return code, first, text


def test_probe_shell_is_resolved() -> None:
    """The argv the probe launches is an absolute, non-WSL shell."""
    mod = load()
    seen: list[list[str]] = []

    def fake_run(argv, **_kw):
        seen.append(list(argv))
        return subprocess.CompletedProcess(argv, 0, "DEFINED\nNONEMPTY\nAB_DIFF\nAC_SAME\n", "")

    stub_run(mod, fake_run)
    stub_shell(mod, r"C:\Program Files\Git\usr\bin\bash.exe")
    code, _, _ = run(mod.main, [])
    check("the gate launches the resolved shell", bool(seen) and seen[0][0].endswith("bash.exe")
          and os.path.isabs(seen[0][0]), f"argv {seen!r}")
    check("...and not the bare word", all(argv[0] != "bash" for argv in seen), f"argv {seen!r}")
    check("...and reports the markers as a pass", code == 0, f"exit {code}")

    for flag in SELF_TEST_SPELLINGS:
        seen.clear()
        code, _, _ = run(mod.main, [flag])
        check(f"the self-test ({flag}) launches the resolved shell for both probes",
              len(seen) == 2 and all(argv[0] != "bash" for argv in seen), f"argv {seen!r}")


def test_shell_on_this_host() -> None:
    """Whatever this host resolves is not the WSL launcher."""
    shell = proctree.find_unix_shell()
    if shell is None:
        print("skip no Unix shell on this host; nothing to resolve")
        return
    check("this host's probe shell is not the WSL launcher",
          not proctree._is_wsl_launcher(shell), shell)


def test_exit_codes() -> None:
    for mode, args in MODES:
        mod = load()
        stub_shell(mod, None)
        code, first, _ = run(mod.main, args)
        check(f"{mode}: no shell is a skip (3), with its reason first",
              code == 3 and "SKIPPED" in first, f"exit {code}, first {first!r}")

        mod = load()
        stub_shell(mod, "/bin/bash")
        mod.run_probe = lambda _src, _tmp, _bash: ("NOSHA\n", "")
        code, first, _ = run(mod.main, args)
        check(f"{mode}: no sha256sum is a skip (3), with its reason first",
              code == 3 and "SKIPPED" in first, f"exit {code}, first {first!r}")

        mod = load()
        stub_shell(mod, "/bin/bash")
        mod.run_probe = lambda _src, _tmp, _bash: ("", "bash: something died\n")
        code, first, text = run(mod.main, args)
        check(f"{mode}: a probe that cannot run is no verdict (2), not a finding",
              code == 2, f"exit {code}, first {first!r}")
        check(f"{mode}: ...and the report carries the probe's stderr",
              "something died" in text, text[-300:])

        mod = load()
        stub_shell(mod, "/bin/bash")
        mod.run_probe = lambda _src, _tmp, _bash: ("", "no answer in 120 s")
        code, _, _ = run(mod.main, args)
        check(f"{mode}: a probe that timed out is no verdict (2)", code == 2, f"exit {code}")


def test_finding_is_still_a_finding() -> None:
    """Live: a `contents()` with no hash line is reported, exit 1."""
    mod = load()
    if proctree.find_unix_shell() is None:
        print("skip no Unix shell on this host; the live case cannot run")
        return
    real = mod.extract_contents

    def sabotaged(text):
        src = real(text)
        return "\n".join(
            line for line in src.split("\n")
            if "sha256sum" not in line and "printf 'sha" not in line
        )

    mod.extract_contents = sabotaged
    code, first, text = run(mod.main, [])
    if code == 3:
        print(f"skip {first}")
        return
    check("a contents() with no hash is a finding (1)", code == 1 and "FAILED" in first,
          f"exit {code}: {text[-400:]}")


def test_live_self_test() -> None:
    """Live: the self-test passes on this host, or says why it cannot run."""
    for flag in SELF_TEST_SPELLINGS:
        mod = load()
        code, first, text = run(mod.main, [flag])
        if proctree.find_unix_shell() is None:
            check(f"with no shell, the live self-test ({flag}) skips with a reason",
                  code == 3 and "SKIPPED" in first, f"exit {code}: {text[-400:]}")
            continue
        check(f"the live self-test ({flag}) passes, or has no sha256sum to grade with",
              code == 0 or (code == 3 and "sha256sum" in first), f"exit {code}: {text[-400:]}")
        # The self-test ran, not the gate: only the self-test prints this.
        check("...and it was the self-test that ran, under a named shell",
              code != 0 or ("probe shell:" in text and "selftest: 3 case(s)" in text),
              text[-400:])


def main() -> int:
    test_probe_shell_is_resolved()
    test_shell_on_this_host()
    test_exit_codes()
    test_finding_is_still_a_finding()
    test_live_self_test()
    print()
    print(f"test-check-cp-diff-sees-nul: {len(FAILURES)} failed")
    return 1 if FAILURES else 0


if __name__ == "__main__":
    sys.exit(main())
