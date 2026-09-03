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


class KeptLog:
    """The log `run_checker` keeps, found by glob rather than by exact name.

    The name carries the shell's pid (`<prog>-<label>.<pid>.log`) so that two
    concurrent runs of one gate cannot share a path -- see the comment on
    `_rc_log` in run-checker.sh. Each driver here is a fresh `sh`, so the pid
    is not knowable from out here and every assertion has to go through a glob.

    The glob is deliberately loose enough to match the *old* `<prog>-<label>.log`
    too. That is not sloppiness, it is what makes group 8 a real regression
    test: with a tight `.*.log` pattern, reverting the fix made this suite fail
    back in group 2 -- on the filename, before the concurrency assertions ever
    ran -- which would have "caught" the revert while proving nothing about the
    race. Loose here, group 8 is the only thing that fails, and it fails for
    the reason it exists. Asserting the exact filename is also what would make
    the pid look like a test-breaking change and tempt someone to remove it.
    """

    def __init__(self, tmp: Path, prog: str, label: str) -> None:
        self._tmp = tmp
        self._pat = f"{prog}-{label}*.log"

    def _all(self) -> list[Path]:
        return sorted(self._tmp.glob(self._pat))

    def exists(self) -> bool:
        return bool(self._all())

    def _one(self) -> Path:
        found = self._all()
        if len(found) != 1:
            raise AssertionError(
                f"expected exactly one {self._pat} in {self._tmp}, "
                f"found {[p.name for p in found]}"
            )
        return found[0]

    def read_text(self, **kw: object) -> str:
        return self._one().read_text(**kw)  # type: ignore[arg-type]

    @property
    def name(self) -> str:
        return self._one().name

    def unlink(self, missing_ok: bool = False) -> None:
        found = self._all()
        if not found and not missing_ok:
            raise FileNotFoundError(self._pat)
        for p in found:
            p.unlink()


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


def write_driver(
    tmp: Path,
    func: str,
    script: Path,
    *args: str,
    configured: bool = True,
    flag: str = "",
    skiplog: Path | None = None,
) -> Path:
    """The one-shot shell script that calls `run_checker` as a caller would.

    Named after the checker it drives rather than a fixed `driver.sh`, because
    group 8 runs two drivers at once and a shared name would have the second
    rewrite the first's script out from under a running `sh`.

    `flag` goes *before* the label, which is where the real call sites put
    `--may-skip=N`; `skiplog` sets `CHECKER_SKIPLOG` in the driver's own shell,
    where `run_checker` -- a function in that same shell -- will read it.

    Both live here rather than in `run` because there is only one place that
    knows this script's shape, and a second one would drift from it. `start`
    already shares this builder for the concurrency tests, and would otherwise
    have silently lacked the option the skip tests exercise.
    """
    driver = tmp / f"driver-{script.stem}.sh"
    driver.write_text(
        "set -u\n"
        + preamble(tmp, configured)
        + (f'CHECKER_SKIPLOG="{skiplog.as_posix()}"\n' if skiplog else "")
        + f"{func}\n"
        + f"run_checker {flag + ' ' if flag else ''}testgate "
        + f'"{sys.executable}" "{script.as_posix()}" '
        + " ".join(args)
        + "\n"
        + 'echo "MARKER-RETURNED rc=$?"\n',
        encoding="utf-8",
    )
    return driver


def run(
    tmp: Path,
    func: str,
    script: Path,
    *args: str,
    configured: bool = True,
    flag: str = "",
    skiplog: Path | None = None,
) -> subprocess.CompletedProcess:
    """Drive `run_checker` once, in a shell, exactly as a caller would.

    `MARKER-RETURNED` is echoed only if the helper *returns*; the no-verdict
    path exits the caller, so its absence is the assertion that it did.

    `flag` is passed as a string rather than a bool deliberately: the tests
    that matter most for `--may-skip` are the ones that hand it a value it must
    refuse, and a bool cannot express `--may-skip=yes`. See `write_driver` for
    where it lands in the generated call.
    """
    driver = write_driver(
        tmp, func, script, *args, configured=configured, flag=flag, skiplog=skiplog
    )
    return subprocess.run(
        ["sh", str(driver)],
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        env=child_env(),
    )


def start(
    tmp: Path,
    func: str,
    script: Path,
    *args: str,
    configured: bool = True,
) -> subprocess.Popen:
    """`run`, but left running, so two drivers can genuinely overlap."""
    driver = write_driver(tmp, func, script, *args, configured=configured)
    return subprocess.Popen(
        ["sh", str(driver)],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
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


# `run_checker <label>` in *command position*, with the label optionally quoted
# because several are interpolated (`"request-deletion-$sha"`). The three
# accepted prefixes are the only shapes boot-test.sh and the hook actually use,
# measured rather than assumed: a bare call, `if run_checker`, and
# `if ! run_checker`. If a fourth shape is ever introduced, the
# "runs every checker through it" count assertion below drops and says so --
# which is the intended failure, and better than a regex that quietly accepts
# anything and goes back to matching prose.
# The `(?:--[a-z-]+=\S+[ \t]+)*` is what steps over option words such as
# `--may-skip=2`. Without it the flag itself was captured as the label, so two
# call sites carrying `--may-skip` collided and "boot-test's labels are
# distinct" failed on a correct tree -- the same phantom-gate shape the
# docstring below describes, arriving this time from a real call rather than
# from prose. A label matcher has to know the call's grammar, not just its
# first word.
LABEL_RE = re.compile(
    r"^[ \t]*(?:if[ \t]+)?(?:![ \t]*)?run_checker[ \t]+"
    r"(?:--[a-z-]+=\S+[ \t]+)*"
    r"\"?([a-z0-9$-]+)",
    re.MULTILINE,
)


def run_checker_labels(text: str) -> list[str]:
    """Every label passed to `run_checker`, ignoring prose that names it.

    Two filters, because the prose that names `run_checker` has twice turned
    up somewhere the previous filter did not look.

    1. Code lines only. The converted gates carry comments saying things like
       "run_checker makes that distinction for every gate now", and reading
       those as calls invented a phantom gate labelled `makes` -- twice, so
       the duplicate-label check below reported a collision that did not
       exist. A structural test that reads comments is testing the comments.

    2. Command position only. That was not enough, and the same bug came back
       wearing different clothes: `check-gates-are-wired`'s own refusal text
       is printed by `echo "  * a run_checker call whose script argument
       could not be resolved," >&2`, on three separate lines. Those are code
       -- filter 1 keeps them, correctly -- but the words inside the string
       are still prose, and they yielded a phantom gate labelled `call`,
       three times, failing the duplicate check on an unmodified tree.

       The generalisation of both: a structural test that reads a file's
       *English* is testing the English. What makes a call a call is that the
       word starts a command, so that is what is matched, and a mention
       anywhere else on the line -- comment, echo, heredoc, error message --
       cannot be one.
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
        log = KeptLog(tmp_root, "pre-push", "testgate")

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
        # The negative half of group 4b's discrimination, and it has to live
        # here rather than there: a patch that printed the launch-failure
        # reading for *every* non-verdict code would satisfy every assertion
        # in 4b while being exactly as wrong as the bug 4b exists to prevent.
        # This is the line that fails on such a patch.
        check("still gives the contention advice for an ordinary code",
              "Re-run it alone before concluding anything" in flat)
        check("does not offer the launch-failure reading",
              "could not execute it" not in flat
              and "could not run it at all" not in flat)
        log.unlink(missing_ok=True)

        # ------------------------------------------------------------------
        # 126 and 127 are the shell's codes for "never launched", and no
        # checker in this tree returns either as a verdict. Advising the
        # reader to re-run a checker alone is right for an OOM kill and wrong
        # here: `Argument list too long` is a limit on the command, not on the
        # machine, so the retry reaches the identical wall. See
        # known-issues.md -> B-A-CHECKER-THAT-CANNOT-BE-LAUNCHED-IS-REPORTED-
        # AS-A-RESOURCE-SHORTAGE.
        print("group 4b: a checker that could not be launched")
        toolong = fake_checker(
            tmp_root,
            "toolong",
            "import sys\n"
            "print('/usr/bin/sh: Argument list too long')\n"
            "sys.exit(126)\n",
        )
        r = run(tmp_root, func, toolong)
        flat = flatten(r.stdout + r.stderr)
        check("aborts the run", r.returncode == 1, f"rc={r.returncode}")
        check("reports the exit code it saw", "exited 126" in flat)
        check("reads 126 as a launch failure", "could not execute it" in flat)
        check("quotes the checker's own first line",
              "Argument list too long" in flat)
        check("says the retry is futile", "identical wall" in flat)
        # The whole point of the fix: the old message ended here with advice
        # to re-run it alone, which for E2BIG costs a full push to learn
        # nothing.
        check("does NOT give the contention advice",
              "Re-run it alone before concluding anything" not in flat)
        log.unlink(missing_ok=True)

        # 127 is deliberately NOT the mirror of 126. A fork() refused by the
        # Windows commit limit surfaces as 127 on this host (boot-history.py,
        # HARNESS_ABORT_EXITS), so the retry advice is right for one of its
        # two causes. The message must offer both readings rather than pick.
        notfound = fake_checker(
            tmp_root,
            "notfound",
            "import sys\nprint('sh: nosuchtool: not found')\nsys.exit(127)\n",
        )
        r = run(tmp_root, func, notfound)
        flat = flatten(r.stdout + r.stderr)
        check("aborts the run", r.returncode == 1, f"rc={r.returncode}")
        check("reports the exit code it saw", "exited 127" in flat)
        check("reads 127 as a launch failure",
              "could not run it at all" in flat)
        check("quotes the checker's own first line", "not found" in flat)
        check("keeps the commit-limit reading too", "commit limit" in flat)
        check("says which output tells them apart", "tells them apart" in flat)
        log.unlink(missing_ok=True)

        # ------------------------------------------------------------------
        # `--may-skip=N` lets one call site declare that N means "I could not
        # look" for its gate only. The tests below are weighted towards the
        # ways it must *refuse*, because the failure that matters is not the
        # option failing to work -- that is obvious on the first run -- but the
        # option working too well and swallowing a finding or a crash.
        print("group 4c: a gate whose call site allows it to skip")
        skiplog = tmp_root / "skips.tsv"
        cannot_look = fake_checker(
            tmp_root,
            "cannot-look",
            "import sys\nprint('libc.a is stale; nothing to grade')\n"
            "sys.exit(2)\n",
        )
        r = run(tmp_root, func, cannot_look, flag="--may-skip=2",
                skiplog=skiplog)
        out = r.stdout + r.stderr
        flat = flatten(out)
        check("returns 0 so the build continues",
              "MARKER-RETURNED rc=0" in out, out.strip()[-300:])
        check("says SKIPPED in as many words", "SKIPPED" in out)
        check("says it is not a pass", "NOT a pass" in flat)
        check("quotes the reason the checker gave",
              "libc.a is stale" in out)
        check("does not claim a crash",
              "never reached a verdict" not in flat)
        check("deletes the log, as for any non-finding", not log.exists())
        check("records the skip where a caller can count it",
              skiplog.is_file()
              and "testgate\t2\t" in skiplog.read_text(encoding="utf-8"),
              skiplog.read_text(encoding="utf-8") if skiplog.is_file()
              else "no skiplog written")
        skiplog.unlink(missing_ok=True)

        # A gate that gets partway before finding it cannot look. This is the
        # ordinary shape, not an exotic one -- a checker validates its inputs,
        # says so, and only then reaches for the instrument that is missing --
        # and the fixture above could not detect it going wrong, because a
        # checker printing a single line has the same first and last line.
        #
        # It caught a real defect: the announcement used `head -n 1`, so
        # `check-shellquote-vs-bash` skipping for want of WSL was reported as
        # "port verified against shellquote.rs". That is a *success* message,
        # naming a subsystem that was never the problem, offered as the reason
        # nothing was checked. The reason a gate gives up is the last thing it
        # says before it does.
        progressed = fake_checker(
            tmp_root,
            "progressed",
            "import sys\n"
            "print('port verified against shellquote.rs')\n"
            "print('cases loaded: 214')\n"
            "print('NO BASH TO ASK -- could not launch wsl', file=sys.stderr)\n"
            "sys.exit(2)\n",
        )
        r = run(tmp_root, func, progressed, flag="--may-skip=2",
                skiplog=skiplog)
        out = r.stdout + r.stderr
        check("quotes the reason, not the last thing that worked",
              "NO BASH TO ASK" in out, out.strip()[-300:])
        check("does not quote an earlier success as the reason",
              "port verified" not in flatten(out).split("NOT a pass")[0]
              .split("SKIPPED")[-1],
              out.strip()[-300:])
        check("records the reason, not the progress, in the skiplog",
              skiplog.is_file()
              and "NO BASH TO ASK" in skiplog.read_text(encoding="utf-8"),
              skiplog.read_text(encoding="utf-8") if skiplog.is_file()
              else "no skiplog written")
        skiplog.unlink(missing_ok=True)

        # A blank last line must not be quoted as the reason either: a checker
        # that ends with a trailing newline would otherwise announce its skip
        # with an empty quote, which reads as "it said nothing" about a gate
        # that said plenty.
        trailing = fake_checker(
            tmp_root,
            "trailing",
            "import sys\nprint('the sysroot is stale')\nprint()\nprint('   ')\n"
            "sys.exit(2)\n",
        )
        r = run(tmp_root, func, trailing, flag="--may-skip=2")
        out = r.stdout + r.stderr
        check("skips blank trailing lines when quoting the reason",
              "the sysroot is stale" in out, out.strip()[-300:])

        # The same exit code, from a crash. This is the case that decides
        # whether the option is safe: a checker that dies of `SystemExit(2)`
        # in a bug exits 2 exactly as one that looked and found nothing to
        # look at, and only the traceback tells them apart.
        crash_with_skip_code = fake_checker(
            tmp_root,
            "crash-two",
            "print('starting')\nraise ValueError('boom')\n",
        )
        r = run(tmp_root, func, crash_with_skip_code, flag="--may-skip=2",
                skiplog=skiplog)
        out = r.stdout + r.stderr
        flat = flatten(out)
        check("a traceback is a crash even with the skip code allowed",
              "MARKER-RETURNED" not in out, out.strip()[-300:])
        check("and it still aborts", r.returncode == 1, f"rc={r.returncode}")
        check("a crash is never recorded as a skip", not skiplog.exists())
        log.unlink(missing_ok=True)

        # Without the flag, exit 2 must still abort -- the whole point of the
        # option being per call site is that it changes nothing anywhere else.
        r = run(tmp_root, func, cannot_look)
        out = r.stdout + r.stderr
        check("exit 2 without the flag still reaches no verdict",
              "MARKER-RETURNED" not in out
              and "never reached a verdict" in flatten(out),
              out.strip()[-300:])
        log.unlink(missing_ok=True)

        # A code the call site did not name is still a crash.
        other_code = fake_checker(
            tmp_root, "other-code", "import sys\nprint('hm')\nsys.exit(3)\n")
        r = run(tmp_root, func, other_code, flag="--may-skip=2")
        check("only the declared code skips; 3 is still no verdict",
              "MARKER-RETURNED" not in (r.stdout + r.stderr),
              (r.stdout + r.stderr).strip()[-300:])
        log.unlink(missing_ok=True)

        # The refusals. Each of these would, if accepted, turn the option into
        # a way to silence a gate rather than to run one.
        for bad, why in (
            ("--may-skip=0", "0 is already a pass"),
            ("--may-skip=1", "1 is already a finding"),
            ("--may-skip=126", "126 is a failed invocation"),
            ("--may-skip=127", "127 is a failed invocation"),
            ("--may-skip=yes", "not a number"),
            ("--may-skip=", "empty"),
        ):
            r = run(tmp_root, func, clean, flag=bad)
            out = r.stdout + r.stderr
            check(f"refuses {bad} ({why})",
                  "MARKER-RETURNED" not in out and r.returncode == 1,
                  out.strip()[-200:])
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
        KeptLog(tmp_root, "checker", "testgate").unlink(missing_ok=True)

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
        KeptLog(tmp_root, "checker", "testgate").unlink(missing_ok=True)

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

        # The extractor is itself a checker, so it gets the treatment every
        # other checker here gets: one input it must find and one it must
        # not. Twice now a mention of `run_checker` in prose has been counted
        # as a call -- once in a comment, once inside an `echo` -- and both
        # times the symptom was a duplicate-label failure on a tree where no
        # duplicate existed. Asserting only that real calls are found would
        # pass in both of those states.
        probe = "\n".join([
            "run_checker alpha \"$py\" scripts/check-a.py",
            "if run_checker beta \"$py\" scripts/check-b.py; then :; fi",
            "if ! run_checker gamma \"$py\" scripts/check-c.py; then :; fi",
            "# run_checker makes that distinction for every gate now",
            'echo "  * a run_checker call whose argument could not be resolved" >&2',
            'echo "Add a run_checker call for the gate, or pin it" >&2',
        ])
        found = run_checker_labels(probe)
        check("the label extractor finds calls in every shape used",
              found == ["alpha", "beta", "gamma"], f"found {found}")
        check("the label extractor ignores run_checker named in prose",
              "makes" not in found and "call" not in found, f"found {found}")

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

        # The extractor must step over option words to find the label. When
        # `--may-skip=2` was first used at two call sites, this matcher read
        # the flag itself as the label, so both "gates" were called
        # `--may-skip` and the distinctness check above failed on a correct
        # tree. Pinned directly rather than left to the real file, so it stays
        # covered if boot-test.sh ever drops back to a single skipping gate --
        # a duplicate check cannot notice a bug that needs two call sites.
        flagged = run_checker_labels(
            'run_checker --may-skip=2 alpha "$py" a.py\n'
            'if ! run_checker --may-skip=2 beta "$py" b.py; then\n'
            "run_checker gamma \"$py\" c.py\n"
        )
        check("the label extractor steps over option words",
              flagged == ["alpha", "beta", "gamma"], repr(flagged))

        # The same invariant for the hook, which until 2026-09-02 was not
        # checked at all -- the assertion above was written for boot-test and
        # never extended, so a label collision at the push boundary (the more
        # expensive of the two to diagnose, since its evidence is what the
        # kept log *is*) would have gone unreported.
        #
        # It cannot simply be `not dupes`, because the hook has one legitimate
        # repeat: `getopt-table` is called from the two arms of an `if/elif`
        # (run_all versus a named list of binaries), so it is one gate spelled
        # twice and exactly one arm ever executes. Nothing is overwritten.
        #
        # Pinned rather than exempted-by-pattern: "duplicates are fine when
        # the calls are mutually exclusive" is true but not decidable from
        # shell text without evaluating it, and a rule the checker cannot
        # actually apply is one that silently permits the real collisions too.
        # A named pin is a claim someone verified once, and a new duplicate --
        # which is the case that loses evidence -- still fails here.
        HOOK_DUPES_OK = {"getopt-table"}
        hook_dupes = sorted({g for g in hook_gates if hook_gates.count(g) > 1})
        unexpected = [g for g in hook_dupes if g not in HOOK_DUPES_OK]
        check("the hook has no unpinned duplicate labels",
              not unexpected, ", ".join(unexpected))
        # The pin is itself a claim about the tree, so it expires when it stops
        # being true rather than sitting there forever protecting nothing.
        stale = [g for g in HOOK_DUPES_OK if g not in hook_dupes]
        check("the duplicate-label pin has no stale entries",
              not stale,
              f"{', '.join(stale)} is pinned but no longer duplicated -- "
              "drop it from HOOK_DUPES_OK")
        # ------------------------------------------------------------------
        # The distinct-label rule group 7 checks is about one invocation
        # overwriting itself. This is the other axis, which no assertion
        # covered until 2026-09-03: two invocations running at once compute the
        # *same* label for the same gate, so before the pid went into the name
        # they shared a path -- and the first to finish clean `rm -f`'d it out
        # from under the other, which had a real refusal to keep. Observed for
        # real from three overlapping pushes of one sha.
        print("group 8: concurrent runs of one gate keep separate logs")
        found = fake_checker(
            tmp_root,
            "slowfound",
            # Long enough that the two overlap for certain rather than by luck.
            "import sys, time\ntime.sleep(1.5)\n"
            "print('src/a.rs: 3 sites')\nsys.exit(1)\n",
        )
        clean_slow = fake_checker(
            tmp_root,
            "slowclean",
            "import time\ntime.sleep(1.5)\nprint('ok -- 0 sites')\n",
        )
        # The failing one starts first so that, under the old naming, the
        # passing one's `rm -f` is what destroys the evidence.
        procs = [
            start(tmp_root, func, found),
            start(tmp_root, func, clean_slow),
        ]
        outs = [p.communicate() for p in procs]
        joined = "".join(o[0] + o[1] for o in outs)
        check("the failing run still returns 1",
              "MARKER-RETURNED rc=1" in joined, joined[-300:])
        check("the passing run still returns 0",
              "MARKER-RETURNED rc=0" in joined, joined[-300:])
        check("neither run lost its log to the other",
              "No such file or directory" not in joined, joined[-300:])
        kept = sorted(tmp_root.glob("pre-push-testgate*.log"))
        check("the failing run's evidence survived", len(kept) == 1,
              f"found {[p.name for p in kept]}")
        if kept:
            check("and it holds that run's findings",
                  "src/a.rs: 3 sites" in kept[0].read_text(encoding="utf-8"))
        for p in kept:
            p.unlink()

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
