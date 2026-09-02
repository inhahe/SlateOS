#!/usr/bin/env python3
"""Tests for `run_checker`, the pre-push hook's checker-invocation helper.

`scripts/hooks/pre-push` runs eleven gates, each of which asks a checker
whether the tree is clean. Until 2026-09-01 every gate asked with a bare
``if ! "$py" "$script" --check``, which cannot distinguish the two things a
Python script says by exiting 1: *I found violations* and *I raised an
exception*. A checker that crashed was therefore reported to the operator in
the gate's own accusing words -- your diagnostic names a file without quoting
it -- and the only remedy the refusal offered was that gate's bypass, which
does not fix anything, it switches off a check that never ran.

That is not a hypothetical. On 2026-09-01 gate 8 refused a push whose tree
passed `quote-names.py --check` cleanly, unchanged, minutes later; two hours
went into hunting a defect in `cp` that was never there. See known-issues.md ->
B-TOOLING-INTERMITTENT-HOST-FAILURES-LOSE-THEIR-OWN-EVIDENCE and
design-decisions.md section 746.

## Why this suite extracts the function instead of reimplementing it

The hook is a `#!/bin/sh` script that git feeds a ref list on stdin and that
runs all eleven gates on the way past; it cannot simply be sourced. The
alternative -- copying `run_checker` into this file and testing the copy -- is
the exact failure these gates warn about elsewhere: the copy passes while the
original rots. So the function is *cut out of the real file* by brace matching
and evaluated. If someone renames or restructures it, extraction fails loudly
rather than testing something that is no longer shipped.

The last group tests the file rather than the function: that no gate still
invokes a checker the old way. That is the regression with a future, since the
next gate added is the one likely to be written in the old shape.
"""

from __future__ import annotations

import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
HOOK = ROOT / "scripts" / "hooks" / "pre-push"

failures: list[str] = []


def check(label: str, ok: bool, detail: str = "") -> None:
    if ok:
        print(f"  ok   {label}")
    else:
        print(f"  FAIL {label}" + (f" -- {detail}" if detail else ""))
        failures.append(label)


def extract_run_checker(text: str) -> str:
    """The `run_checker() { ... }` block, cut out by brace matching.

    Matches the closing brace at column 0, which is this file's style for a
    function body and is unambiguous: every inner `}` in the body is indented.
    """
    lines = text.splitlines()
    start = None
    for i, line in enumerate(lines):
        if line.startswith("run_checker() {"):
            start = i
            break
    if start is None:
        raise SystemExit(
            "cannot find `run_checker() {` in scripts/hooks/pre-push -- the "
            "helper was renamed or restructured, and this suite is testing "
            "nothing. Update the extraction, do not delete the test."
        )
    for j in range(start + 1, len(lines)):
        if lines[j] == "}":
            return "\n".join(lines[start : j + 1])
    raise SystemExit("run_checker's body has no closing brace at column 0")


def fake_checker(tmp: Path, name: str, body: str) -> Path:
    p = tmp / f"{name}.py"
    p.write_text(body, encoding="utf-8")
    return p


def run(tmp: Path, func: str, script: Path, *args: str) -> subprocess.CompletedProcess:
    """Drive `run_checker` once, in a shell, exactly as the hook would.

    `MARKER-RETURNED` is echoed only if the helper *returns*; the no-verdict
    path exits the hook, so its absence is the assertion that it did.
    """
    driver = tmp / "driver.sh"
    driver.write_text(
        "set -u\n"
        f'py="{sys.executable}"\n'
        f'hook_logdir="{tmp.as_posix()}"\n'
        f"{func}\n"
        f'run_checker testgate "{script.as_posix()}" {" ".join(args)}\n'
        "echo \"MARKER-RETURNED rc=$?\"\n",
        encoding="utf-8",
    )
    return subprocess.run(
        ["sh", str(driver)],
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
    )


def main() -> int:
    if not HOOK.is_file():
        print(f"skip: {HOOK} not in this checkout")
        return 0
    if shutil.which("sh") is None:
        print("skip: no `sh` on PATH")
        return 0

    text = HOOK.read_text(encoding="utf-8")
    func = extract_run_checker(text)

    tmp_root = Path(tempfile.mkdtemp(prefix="prepush-runchecker-"))
    try:
        log = tmp_root / "pre-push-testgate.log"

        # ------------------------------------------------------------------
        print("group 1: a clean checker")
        clean = fake_checker(tmp_root, "clean", "print('ok -- 0 sites')\n")
        r = run(tmp_root, func, clean)
        out = r.stdout + r.stderr
        check("returns 0", "MARKER-RETURNED rc=0" in out, out.strip()[-300:])
        check("echoes the checker's output", "ok -- 0 sites" in out)
        check("deletes the log of a passing gate", not log.exists())

        # ------------------------------------------------------------------
        print("group 2: a checker that found something")
        found = fake_checker(
            tmp_root,
            "found",
            "import sys\nprint('src/a.rs: 3 sites')\nsys.exit(1)\n",
        )
        r = run(tmp_root, func, found)
        out = r.stdout + r.stderr
        check("returns 1 so the gate prints its own refusal",
              "MARKER-RETURNED rc=1" in out, out.strip()[-300:])
        check("echoes the findings", "src/a.rs: 3 sites" in out)
        check("keeps the log", log.exists())
        check("names where the log was kept", "full output kept at" in out)
        check("the kept log holds the findings",
              "src/a.rs: 3 sites" in log.read_text(encoding="utf-8"))
        check("does not claim a crash", "never reached a verdict" not in out)
        log.unlink(missing_ok=True)

        # ------------------------------------------------------------------
        # The case the whole helper exists for: exit 1, like a finding, but
        # from an uncaught exception rather than a verdict.
        print("group 3: a checker that raised")
        crashed = fake_checker(
            tmp_root,
            "crashed",
            "print('scanning...')\nraise MemoryError('cannot allocate')\n",
        )
        r = run(tmp_root, func, crashed)
        out = r.stdout + r.stderr
        check("does not return to the gate", "MARKER-RETURNED" not in out,
              out.strip()[-300:])
        check("aborts the push", r.returncode == 1, f"rc={r.returncode}")
        check("says no verdict was reached", "never reached a verdict" in out)
        check("says it is not a finding", "NOT a finding" in out)
        check("warns against the bypass", "bypass" in out)
        check("keeps the log", log.exists())
        check("the kept log holds the traceback",
              "MemoryError" in log.read_text(encoding="utf-8"))
        check("names the log path", log.name in out)
        check("shows how to re-run it", "crashed.py" in out)
        check("points at the known-issues entry",
              "B-TOOLING-INTERMITTENT-HOST-FAILURES-LOSE-THEIR-OWN-EVIDENCE" in out)
        log.unlink(missing_ok=True)

        # ------------------------------------------------------------------
        # A checker invoked wrongly by the hook is also not the operator's
        # defect. Several checkers use exit 2 for a usage error.
        print("group 4: a checker that exited 2")
        usage = fake_checker(
            tmp_root,
            "usage",
            "import sys\nprint('--fix needs at least one path')\nsys.exit(2)\n",
        )
        r = run(tmp_root, func, usage)
        out = r.stdout + r.stderr
        check("does not return to the gate", "MARKER-RETURNED" not in out)
        check("aborts the push", r.returncode == 1, f"rc={r.returncode}")
        check("says no verdict was reached", "never reached a verdict" in out)
        check("reports the exit code it saw", "exited 2" in out)
        log.unlink(missing_ok=True)

        # ------------------------------------------------------------------
        # Gate 11 sets IFS to a newline to split a path list. `$*` joins on
        # IFS, so the re-run line would come out one argument per line.
        print("group 5: the re-run line survives a caller's IFS")
        driver = tmp_root / "driver.sh"
        driver.write_text(
            "set -u\n"
            f'py="{sys.executable}"\n'
            f'hook_logdir="{tmp_root.as_posix()}"\n'
            f"{func}\n"
            "IFS='\n'\n"
            f'run_checker testgate "{crashed.as_posix()}" --check one two\n',
            encoding="utf-8",
        )
        r = subprocess.run(["sh", str(driver)], capture_output=True, text=True,
                           encoding="utf-8", errors="replace")
        out = r.stdout + r.stderr
        check("the command is on one line",
              "--check one two" in out,
              "\n".join(li for li in out.splitlines() if "--check" in li))
        log.unlink(missing_ok=True)

        # ------------------------------------------------------------------
        # Tests the file, not the function: the next gate added is the one
        # likely to be written in the old shape.
        print("group 6: every gate goes through run_checker")
        body_start = text.index("run_checker() {")
        body_end = text.index("\n}\n", body_start) + 3
        outside = text[:body_start] + text[body_end:]
        # `"$py" "$something"` is the old shape. Comments quoting it while
        # explaining why it is gone are not invocations.
        stray = [
            (n, li)
            for n, li in enumerate(outside.splitlines(), 1)
            if re.search(r'"\$py"\s+"\$', li) and not li.lstrip().startswith("#")
        ]
        check("no gate still invokes a checker directly", not stray,
              "; ".join(f"{li.strip()}" for _, li in stray))

        gates = re.findall(r"\brun_checker\s+([a-z0-9-]+)", outside)
        check("every gate's call is named", all(gates),
              f"found {len(gates)} calls")
        check("the names are distinct per checker invocation",
              len(gates) >= 14, f"only {len(gates)} run_checker calls")
    finally:
        shutil.rmtree(tmp_root, ignore_errors=True)

    print()
    if failures:
        print(f"FAILED: {len(failures)} check(s): {', '.join(failures)}")
        return 1
    print("all checks pass")
    return 0


if __name__ == "__main__":
    sys.exit(main())
