#!/usr/bin/env python3
"""Tests for `run_checker`, the shared checker-invocation helper.

`scripts/run-checker.sh` is sourced by both gate boundaries in this project --
`scripts/hooks/pre-push` (eleven gates) and `scripts/boot-test.sh` (twenty-nine
checker invocations). Until 2026-09-01 every one of them asked with a bare
``if ! "$py" "$script" --check``, which cannot distinguish the two things a
Python script says by exiting 1: *I found violations* and *I raised an
exception*. A checker that crashed was therefore reported to the operator in
the gate's own accusing words, and the remedy each refusal went on to offer --
bypass the gate, rewrite the assertion -- does not fix anything, because the
check never ran.

That is not a hypothetical, and it happened at both boundaries on the same day:

* **pre-push gate 8** refused a push whose tree passed `quote-names.py --check`
  cleanly, unchanged, minutes later; two hours went into hunting a defect in
  `cp` that was never there.
* **boot-test** printed *"Each report above is a self-test assertion demanding
  text that no string literal in the kernel can produce"* over an empty report
  list, after `check-selftest-format-wording.py` died of `MemoryError`.

See known-issues.md -> B-TOOLING-INTERMITTENT-HOST-FAILURES-LOSE-THEIR-OWN-EVIDENCE
and design-decisions.md section 746.

## Why this suite extracts the function instead of reimplementing it

The library could be sourced directly, but the thing worth pinning is not "does
a copy of this logic behave" -- it is "does the shipped file behave". Copying
`run_checker` into this suite is the exact failure the gates warn about
elsewhere: the copy passes while the original rots. So the function is *cut out
of the real file* by brace matching and evaluated. If someone renames or
restructures it, extraction fails loudly rather than testing something that is
no longer shipped.

The last two groups test the *callers* rather than the function: that neither
boundary still invokes a checker the old way, and that both actually source the
library. That is the regression with a future, since the next gate added is the
one likely to be written in the old shape.
"""

from __future__ import annotations

import os
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
LIB = ROOT / "scripts" / "run-checker.sh"
HOOK = ROOT / "scripts" / "hooks" / "pre-push"
BOOT = ROOT / "scripts" / "boot-test.sh"

failures: list[str] = []


def check(label: str, ok: bool, detail: str = "") -> None:
    if ok:
        print(f"  ok   {label}")
    else:
        print(f"  FAIL {label}" + (f" -- {detail}" if detail else ""))
        failures.append(label)


def extract_run_checker(text: str) -> str:
    """The `run_checker() { ... }` block, cut out by brace matching.

    Matches the closing brace at column 0, which is the file's style for a
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
            "cannot find `run_checker() {` in scripts/run-checker.sh -- the "
            "helper was renamed or restructured, and this suite is testing "
            "nothing. Update the extraction, do not delete the test."
        )
    for j in range(start + 1, len(lines)):
        if lines[j] == "}":
            return "\n".join(lines[start : j + 1])
    raise SystemExit("run_checker's body has no closing brace at column 0")


def child_env() -> dict[str, str]:
    """This process's environment with every `CHECKER_*` setting removed.

    Every driver below is run under this rather than under a plain inherited
    environment, because the settings the library reads are *exported* by at
    least one real caller and would otherwise leak in.

    That is not a precaution; it is a bug this suite already had. `boot-test.sh`
    does `export CHECKER_PROG CHECKER_REFUSING CHECKER_LOGDIR` before sourcing
    the library, so every suite its tooling loop runs inherits
    `CHECKER_PROG=boot-test` and `CHECKER_REFUSING=build`. Group 6 drives the
    helper from a script that deliberately sets *neither*, to prove each has a
    default -- but under boot-test that script was not an unconfigured caller at
    all. It got `boot-test: REFUSING to build`, the two default strings were
    absent, and the group failed.

    The failure mode is the worst shape available: the suite passed run by hand
    and failed only inside the boot test, where a Python suite's output is one
    line in a forty-minute log. A test whose verdict depends on who invoked it
    is not testing the library.

    Removing the names is right where clearing them to `""` would not be: the
    library's fallbacks are `${CHECKER_PROG:-checker}`, and `:-` treats empty
    and unset alike, so an empty value would happen to work today and would
    stop working the moment a fallback is written `${CHECKER_PROG-checker}`.
    Unset is the state group 6 is about.
    """
    return {k: v for k, v in os.environ.items() if not k.startswith("CHECKER_")}


def fake_checker(tmp: Path, name: str, body: str) -> Path:
    p = tmp / f"{name}.py"
    p.write_text(body, encoding="utf-8")
    return p


def preamble(tmp: Path, configured: bool) -> str:
    """The `CHECKER_*` settings a caller makes before sourcing the library.

    `configured` false is the other half of the contract: every variable has a
    default, because a caller that forgets one should get a slightly blunter
    message rather than a `set -u` abort in the middle of a refusal.
    """
    if not configured:
        return f'CHECKER_LOGDIR="{tmp.as_posix()}"\n'
    return (
        "CHECKER_PROG=pre-push\n"
        f'CHECKER_LOGDIR="{tmp.as_posix()}"\n'
        "CHECKER_REFUSING=push\n"
        'CHECKER_NOTE="\n'
        'The gate\'s bypass is the wrong reaction to it."\n'
    )


def run(
    tmp: Path,
    func: str,
    script: Path,
    *args: str,
    configured: bool = True,
) -> subprocess.CompletedProcess:
    """Drive `run_checker` once, in a shell, exactly as a caller would.

    `MARKER-RETURNED` is echoed only if the helper *returns*; the no-verdict
    path exits the caller, so its absence is the assertion that it did.
    """
    driver = tmp / "driver.sh"
    driver.write_text(
        "set -u\n"
        + preamble(tmp, configured)
        + f"{func}\n"
        + f'run_checker testgate "{sys.executable}" "{script.as_posix()}" '
        + " ".join(args)
        + "\n"
        + 'echo "MARKER-RETURNED rc=$?"\n',
        encoding="utf-8",
    )
    return subprocess.run(
        ["sh", str(driver)],
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        env=child_env(),
    )


def caller_text(path: Path) -> str:
    """A caller with its `run_checker` call lines' comments left in place."""
    return path.read_text(encoding="utf-8")


def flatten(text: str) -> str:
    """`text` with every run of whitespace collapsed to one space.

    The refusal message is a hand-wrapped paragraph, so where its line breaks
    fall is a matter of how long the sentences happen to be -- and re-wording
    one sentence moves the breaks in the next. Asserting `"NOT a finding" in
    out` against the raw text therefore fails the moment an *unrelated* edit
    pushes those three words across a line boundary, which is exactly what
    happened here: a rewrap split `NOT\\na finding` and the suite reported that
    the message no longer said it was not a finding. That is a false report
    about the one sentence this whole library exists to print.

    Use this for assertions about *wording*. Do not use it for assertions about
    *layout* -- group 5 checks that the re-run line is on a single line, and
    flattening would make that check pass unconditionally.
    """
    return " ".join(text.split())


def code_lines(text: str) -> list[str]:
    """The caller's lines with whole-line comments dropped.

    Both assertions below read shell syntax out of a file that spends most of
    itself explaining that syntax in English, so both have to agree on what is
    code. Whole-line only: a trailing `#` inside a heredoc or a string is not a
    comment, and some of the text these files print is the very text other
    assertions are about.
    """
    return [li for li in text.splitlines() if not li.lstrip().startswith("#")]


# `run_checker <label>`, with the label optionally quoted because several are
# interpolated (`"request-deletion-$sha"`).
LABEL_RE = re.compile(r"\brun_checker\s+\"?([a-z0-9$-]+)")


def run_checker_labels(text: str) -> list[str]:
    """Every label passed to `run_checker`, ignoring prose that names it.

    Read off code lines only. The converted gates carry comments saying things
    like "run_checker makes that distinction for every gate now", and reading
    those as calls invented a phantom gate labelled `makes` -- twice, so the
    duplicate-label check below reported a collision that did not exist. A
    structural test that reads comments is testing the comments.
    """
    return LABEL_RE.findall("\n".join(code_lines(text)))


# Python invocations in a gate that are deliberately NOT checkers, with the
# reason, keyed by a substring of the line.
#
# The rule this list is an exception to is about *verdicts*: `run_checker`
# exists because a checker's exit status becomes the gate's answer, and 1 means
# two different things (found something / fell over). An invocation whose status
# is not an answer about the tree is outside that rule, and routing it through
# the helper would be actively wrong -- `run_checker` `exit`s the whole hook on
# a non-1 status, which would turn a recoverable failure into a refused push.
#
# A reason per entry, for the same reason `multicall-aliases.py`'s IGNORE table
# carries one: an unexplained exemption list is indistinguishable from a list of
# defects somebody wanted to stop seeing. `test_the_exempted_invocations_are_
# actually_handled` below is what stops the reason being merely asserted.
NOT_A_CHECKER = {
    '"$fmt_gittree" materialise':
        "gate 7's mirror builder. It is asked to *produce files*, not to judge "
        "the tree, and the gate handles its failure itself by falling back to "
        "one `git cat-file` per file. Through `run_checker` that fallback "
        "would be unreachable: a failure would exit the hook.",
}


def stray_direct_invocations(text: str, pattern: str) -> list[str]:
    """Lines that run a checker without going through the helper.

    Comments are excluded: several of them quote the old shape while explaining
    why it is gone, and a structural assertion satisfied by prose tests prose.
    Lines that *do* name `run_checker` are the new shape and carry the
    interpreter as an argument, so they match the old pattern too.

    `NOT_A_CHECKER` lines are excluded too -- see that table for why, and for
    why the exemption is a named list rather than a looser pattern.
    """
    return [
        li.strip()
        for li in code_lines(text)
        if re.search(pattern, li) and "run_checker" not in li
        and not any(k in li for k in NOT_A_CHECKER)
    ]


def exemption_is_handled(text: str, key: str) -> str:
    """Why `key`'s invocation may skip `run_checker`, verified rather than asserted.

    The exemption's whole justification is that the gate handles the failure
    itself. That is a claim about the hook, so it is checked against the hook:
    the invocation must open an `if`, and that `if` must have an `else` -- the
    branch that runs when it fails. An exemption whose handler was deleted, or
    whose line no longer exists at all, is a hole in the rule rather than an
    exception to it, and reads exactly like one that is still honoured.

    Returns "" when the exemption holds, or a sentence saying what is wrong.
    """
    lines = code_lines(text)
    hits = [i for i, li in enumerate(lines) if key in li]
    if not hits:
        return "no such invocation in the hook (a stale exemption)"
    if len(hits) > 1:
        return f"{len(hits)} invocations match, so the exemption is not specific"
    start = hits[0]
    if not lines[start].strip().startswith("if "):
        return "the invocation does not open an `if`, so nothing handles a failure"
    depth = 1
    for li in lines[start + 1:]:
        stripped = li.strip()
        if stripped.startswith("if ") or stripped == "if":
            depth += 1
        elif stripped == "fi" or stripped.startswith("fi "):
            depth -= 1
            if depth == 0:
                return "the `if` has no `else`, so a failure is silently ignored"
        elif depth == 1 and stripped == "else":
            return ""
    return "the `if` is never closed"


def main() -> int:
    for path in (LIB, HOOK, BOOT):
        if not path.is_file():
            print(f"skip: {path} not in this checkout")
            return 0
    if shutil.which("sh") is None:
        print("skip: no `sh` on PATH")
        return 0

    func = extract_run_checker(LIB.read_text(encoding="utf-8"))

    tmp_root = Path(tempfile.mkdtemp(prefix="runchecker-"))
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
        check("does not claim a crash",
              "never reached a verdict" not in flatten(out))
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
        flat = flatten(out)
        check("does not return to the gate", "MARKER-RETURNED" not in out,
              out.strip()[-300:])
        check("aborts the run", r.returncode == 1, f"rc={r.returncode}")
        check("says no verdict was reached", "never reached a verdict" in flat)
        check("says it is not a finding", "NOT a finding" in flat)
        check("says the output above is not a finding either",
              "nothing printed above it is one either" in flat)
        check("prints the caller's note", "bypass" in flat)
        check("uses the caller's refusal verb", "REFUSING to push" in flat)
        check("keeps the log", log.exists())
        check("the kept log holds the traceback",
              "MemoryError" in log.read_text(encoding="utf-8"))
        check("names the log path", log.name in out)
        check("shows how to re-run it", "crashed.py" in out)
        check("points at the known-issues entry",
              "B-TOOLING-INTERMITTENT-HOST-FAILURES-LOSE-THEIR-OWN-EVIDENCE" in out)
        log.unlink(missing_ok=True)

        # ------------------------------------------------------------------
        # A checker invoked wrongly by the caller is also not the operator's
        # defect. Several checkers use exit 2 for a usage error.
        print("group 4: a checker that exited 2")
        usage = fake_checker(
            tmp_root,
            "usage",
            "import sys\nprint('--fix needs at least one path')\nsys.exit(2)\n",
        )
        r = run(tmp_root, func, usage)
        out = r.stdout + r.stderr
        flat = flatten(out)
        check("does not return to the gate", "MARKER-RETURNED" not in out)
        check("aborts the run", r.returncode == 1, f"rc={r.returncode}")
        check("says no verdict was reached", "never reached a verdict" in flat)
        check("reports the exit code it saw", "exited 2" in flat)
        log.unlink(missing_ok=True)

        # ------------------------------------------------------------------
        # The hook's doc-links gate sets IFS to a newline to split a path list.
        # `$*` joins on IFS, so the re-run line would come out one argument per
        # line.
        print("group 5: the re-run line survives a caller's IFS")
        driver = tmp_root / "driver.sh"
        driver.write_text(
            "set -u\n"
            + preamble(tmp_root, True)
            + f"{func}\n"
            + "IFS='\n'\n"
            + f'run_checker testgate "{sys.executable}" '
            + f'"{crashed.as_posix()}" --check one two\n',
            encoding="utf-8",
        )
        r = subprocess.run(["sh", str(driver)], capture_output=True, text=True,
                           encoding="utf-8", errors="replace",
                           env=child_env())
        out = r.stdout + r.stderr
        check("the command is on one line",
              "--check one two" in out,
              "\n".join(li for li in out.splitlines() if "--check" in li))
        log.unlink(missing_ok=True)

        # ------------------------------------------------------------------
        # Every setting has a default, so a caller that sets none still gets a
        # coherent refusal rather than a `set -u` abort inside one.
        print("group 6: an unconfigured caller still gets a coherent message")
        r = run(tmp_root, func, crashed, configured=False)
        flat = flatten(r.stdout + r.stderr)
        check("still aborts", r.returncode == 1, f"rc={r.returncode}")
        check("falls back to a generic verb", "REFUSING to continue" in flat)
        check("falls back to a generic prefix", "checker: REFUSING" in flat)
        (tmp_root / "checker-testgate.log").unlink(missing_ok=True)

        # The same three assertions again, with this process's own environment
        # carrying the settings -- which is not a hypothetical environment but
        # the one the boot test runs this suite in.
        #
        # THE LIBRARY IS NOT AT FAULT HERE, AND NO ASSERTION BELOW IS ABOUT IT.
        # `boot-test.sh` *exports* `CHECKER_PROG=boot-test` and
        # `CHECKER_REFUSING=build` before sourcing the library, so a shell it
        # spawns is genuinely a configured caller; a sourced shell function has
        # no way to tell a setting its caller exported from one its caller's
        # caller did, and should not try. What was wrong was this suite, which
        # asserted the *defaults* from a driver that could not reach them. It
        # therefore passed run by hand and failed only inside a forty-minute
        # log -- the worst place available for a Python suite's one line of
        # output to go red.
        #
        # So this pins the property that failed, which is the suite's own:
        # its verdict does not depend on who invoked it. It is also the only
        # thing that can notice if `env=child_env()` is ever dropped from
        # `run` -- without it the scrubbing is indistinguishable from its
        # own absence.
        saved = {k: os.environ.get(k) for k in ("CHECKER_PROG", "CHECKER_REFUSING")}
        os.environ["CHECKER_PROG"] = "leaked"
        os.environ["CHECKER_REFUSING"] = "leak"
        try:
            r = run(tmp_root, func, crashed, configured=False)
        finally:
            for k, v in saved.items():
                if v is None:
                    os.environ.pop(k, None)
                else:
                    os.environ[k] = v
        flat = flatten(r.stdout + r.stderr)
        check("an ambient CHECKER_PROG does not reach the driver",
              "checker: REFUSING" in flat and "leaked" not in flat, flat[:200])
        check("an ambient CHECKER_REFUSING does not either",
              "REFUSING to continue" in flat, flat[:200])
        (tmp_root / "checker-testgate.log").unlink(missing_ok=True)

        # ------------------------------------------------------------------
        # Tests the callers, not the function: the next gate added is the one
        # likely to be written in the old shape.
        print("group 7: both boundaries go through run_checker")
        hook = caller_text(HOOK)
        boot = caller_text(BOOT)

        stray = stray_direct_invocations(hook, r'"\$py"\s+"\$')
        check("no pre-push gate invokes a checker directly", not stray,
              "; ".join(stray))
        for key in sorted(NOT_A_CHECKER):
            why = exemption_is_handled(hook, key)
            check(f"the exemption for `{key}` still describes the hook",
                  why == "", why)
        stray = stray_direct_invocations(boot, r'"\$py"\s+"\$PROJECT_ROOT/scripts/')
        check("no boot-test gate invokes a checker directly", not stray,
              "; ".join(stray))

        check("the hook sources the library", "run-checker.sh" in hook)
        check("boot-test sources the library", "run-checker.sh" in boot)

        hook_gates = run_checker_labels(hook)
        check("every pre-push call is named", all(hook_gates),
              f"found {len(hook_gates)} calls")
        check("the hook still runs every gate through it",
              len(hook_gates) >= 14, f"only {len(hook_gates)} run_checker calls")

        boot_gates = run_checker_labels(boot)
        check("every boot-test call is named", all(boot_gates),
              f"found {len(boot_gates)} calls")
        check("boot-test runs every checker through it",
              len(boot_gates) >= 25, f"only {len(boot_gates)} run_checker calls")

        # A label is what names the kept log, so two invocations sharing one
        # would have the second delete the first's evidence.
        dupes = sorted({g for g in boot_gates if boot_gates.count(g) > 1})
        check("boot-test's labels are distinct", not dupes, ", ".join(dupes))
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
