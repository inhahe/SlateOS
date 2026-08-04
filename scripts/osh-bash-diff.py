#!/usr/bin/env python3
"""Differential tester: run a corpus of shell snippets through `osh` and a
reference `bash`, and report every divergence in stdout, stderr or exit status.

`osh` (userspace/oils) is a bash-compatible shell, and the way its fidelity gaps
have actually been found so far is by hand: write a snippet, run it under real
bash, run it under osh, eyeball the difference. This automates exactly that loop
so the same comparison can be replayed after every change instead of once.

Corpus
------
Cases live in `userspace/oils/tests/corpus/*.sh`; one case per file, run with
`<shell> case.sh` in a fresh temporary working directory (so a case may create
files without disturbing the next one). Three magic comments are recognised:

    # STDIN: <text>          feed <text> plus a newline to the shell's stdin
                             (repeatable — each line appends)
    # EXPECT-DIFF: <reason>  this case is *known* to differ; the run fails if it
                             stops differing (so stale waivers can't hide a fix)
    # TIMEOUT: <seconds>     this case needs longer than the 20s default (it
                             waits on jobs, sleeps, etc.); a case that exceeds
                             its budget twice in a row is reported as a hang

Reference shell
---------------
Defaults to MSYS/Git-for-Windows bash on the dev host, overridable with
`--bash`. Note that MSYS bash differs from real Linux bash in a few documented
places (C-locale byte-wise string ops, signal numbering, exit 127 for fatal
expansion errors) — see the TD-OILS-* "probe artifact" entries in
`known-issues.md`. Cases that hit those need an `# EXPECT-DIFF` waiver.

Usage
-----
    python scripts/osh-bash-diff.py                 # run the whole corpus
    python scripts/osh-bash-diff.py -k trap         # only cases matching "trap"
    python scripts/osh-bash-diff.py -v              # show output for every case

Exit status: 0 if every case matched (or diverged exactly as waived), 1 if any
case diverged unexpectedly or a waived case unexpectedly matched, 2 on a setup
error (no osh binary, no reference bash).

Failure reports
---------------
Every failing case is also written in full to
`target/dvscratch/corpus-failures/<timestamp>/<case>.txt`. The stdout report is
easy to lose — it is usually piped through `tail`, and a *flaky* case that fails
one run in four then passes on demand leaves nothing to work from. The report
file survives that: it holds both shells' complete stdout, stderr and status, so
the next occurrence of an intermittent divergence is captured whether or not
anyone was watching.

Each run gets its own timestamped directory, and a run with no failures creates
none. That matters precisely for the flaky case: the reflex on seeing `1 failed`
is to run the harness again, and a shared directory emptied at the start of each
run would have the re-run — the one that passes — delete the evidence. The
newest twenty runs are kept; older ones are retired.

Load
----
A full sweep is minutes of two shells starting thousands of children, so the
host can run out of room for one. Both a timeout and a failure to *start* a
child (Windows `ERROR_COMMITMENT_LIMIT`, POSIX `EAGAIN`) are treated as a
failed measurement rather than an answer, and the case is measured once more;
only the second failure is reported, and it says which kind it was.
"""

from __future__ import annotations

import argparse
import os
import shutil
import subprocess
import sys
import tempfile
import time
from dataclasses import dataclass
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import proctree  # noqa: E402  (needs the path above)

REPO = Path(__file__).resolve().parent.parent
CORPUS = REPO / "userspace" / "oils" / "tests" / "corpus"
# Where a failing case's full report is left behind; see the module docstring.
REPORTS = REPO / "target" / "dvscratch" / "corpus-failures"
# This run's own subdirectory of it, created lazily by `run_report_dir` so a
# run with nothing to report leaves nothing behind.
_RUN_REPORTS: Path | None = None

# Where the host build puts osh. The *newest* existing one wins: a stale
# `release/` build silently testing week-old behaviour is the easiest way to
# make this whole harness lie.
OSH_CANDIDATES = [
    REPO / "target" / "x86_64-pc-windows-gnu" / "release" / "osh.exe",
    REPO / "target" / "x86_64-pc-windows-gnu" / "debug" / "osh.exe",
    REPO / "target" / "release" / "osh",
    REPO / "target" / "debug" / "osh",
]

BASH_CANDIDATES = [
    Path("C:/Program Files/Git/usr/bin/bash.exe"),
    Path("/usr/bin/bash"),
    Path("/bin/bash"),
]

# A case that takes longer than this is treated as a hang, not a divergence.
# A case that legitimately needs longer — one that waits on jobs, say — declares
# its own budget with a `# TIMEOUT: N` line rather than pushing this default up
# for all 150 cases, which would turn every real hang into a long stall.
CASE_TIMEOUT = 20


@dataclass
class Case:
    path: Path
    source: str
    stdin: str
    expect_diff: str | None
    timeout: int

    @property
    def name(self) -> str:
        return self.path.stem


@dataclass
class Run:
    stdout: str
    stderr: str
    status: int
    timed_out: bool = False


def load_cases(pattern: str | None) -> list[Case]:
    cases: list[Case] = []
    for path in sorted(CORPUS.glob("*.sh")):
        if pattern and pattern not in path.stem:
            continue
        source = path.read_text(encoding="utf-8")
        stdin_lines: list[str] = []
        expect_diff: str | None = None
        timeout = CASE_TIMEOUT
        for line in source.splitlines():
            stripped = line.strip()
            if stripped.startswith("# STDIN:"):
                stdin_lines.append(stripped[len("# STDIN:"):].lstrip())
            elif stripped.startswith("# EXPECT-DIFF:"):
                expect_diff = stripped[len("# EXPECT-DIFF:"):].strip()
            elif stripped.startswith("# TIMEOUT:"):
                field = stripped[len("# TIMEOUT:"):].strip()
                if not field.isdigit() or int(field) < 1:
                    sys.exit(
                        f"osh-bash-diff: {path.name}: `# TIMEOUT: {field}` is not a"
                        " positive number of seconds"
                    )
                timeout = int(field)
        stdin = "".join(f"{line}\n" for line in stdin_lines)
        cases.append(
            Case(
                path=path,
                source=source,
                stdin=stdin,
                expect_diff=expect_diff,
                timeout=timeout,
            )
        )
    return cases


def find_shell(explicit: str | None, candidates: list[Path], what: str, newest: bool = False) -> Path:
    if explicit:
        path = Path(explicit)
        if not path.exists():
            sys.exit(f"osh-bash-diff: {what} not found: {path}")
        return path
    present = [c for c in candidates if c.exists()]
    if present:
        return max(present, key=lambda p: p.stat().st_mtime) if newest else present[0]
    on_path = shutil.which(what)
    if on_path:
        return Path(on_path)
    sys.exit(
        f"osh-bash-diff: no {what} found (looked in "
        + ", ".join(str(c) for c in candidates)
        + "). Build it or pass an explicit path."
    )


def run_case(shell: Path, case: Case) -> Run:
    """Run one case with `shell`, in a throwaway cwd, and capture its output."""
    with tempfile.TemporaryDirectory(prefix="oshdiff-") as workdir:
        script = Path(workdir) / "case.sh"
        # Byte-for-byte, *not* `write_text`: on Windows that would translate every
        # LF to CRLF, and the two shells disagree about stray CRs — which would
        # show up as a spurious divergence in every multi-line case.
        script.write_bytes(case.source.encode("utf-8"))
        # A predictable environment: the case's own output must not depend on
        # the invoking shell's variables, prompt or locale.
        env = dict(os.environ)
        env.update(
            {
                "PS1": "$ ",
                "PS2": "> ",
                "LC_ALL": "C",
                "BASH_ENV": "",
                "ENV": "",
                "COLUMNS": "80",
            }
        )
        # `proctree.run_captured`, not `subprocess.run(timeout=…)`: a case is
        # free to leave a process behind — a `&` job, an interposed `#!`
        # interpreter — and on Windows such a grandchild inherits the capture
        # pipes. `subprocess.run` kills only the shell and then drains those
        # pipes with no timeout of its own, so one surviving grandchild wedges
        # the whole sweep for good (observed: the runner sitting at 0% CPU,
        # childless, hours past a 20s budget). Killing the tree first is what
        # closes the write ends. See `proctree.py`.
        res = proctree.run_captured(
            [str(shell), "case.sh"],
            cwd=workdir,
            input=case.stdin.encode(),
            timeout=case.timeout,
            env=env,
        )
        if res.timed_out:
            return Run(
                stdout="",
                stderr=f"<timed out after {case.timeout}s>",
                status=-1,
                timed_out=True,
            )
    return Run(
        stdout=res.stdout.decode("utf-8", "replace"),
        stderr=res.stderr.decode("utf-8", "replace"),
        status=res.returncode,
    )


# Windows `ERROR_COMMITMENT_LIMIT` (1455): the system could not reserve backing
# store for a new process. It is a host limit, not an answer — see `starved`.
_STARVED = (
    "os error 1455",
    "The paging file is too small",
    "Resource temporarily unavailable",  # POSIX EAGAIN from fork(2)
)


def starved(run: Run) -> bool:
    """Whether this run failed to *start* a child rather than answering.

    The cases that keep several children alive at once are the first to be
    refused a process when the host is out of commit charge or process slots,
    and the refusal reaches us as an ordinary error message from the shell —
    so without this check it reads as a divergence from the other shell, whose
    children were started at a different moment and survived.
    """
    return any(m in run.stdout or m in run.stderr for m in _STARVED)


def measure(shell: Path, case: Case) -> Run:
    """Run one case, retrying once if it times out or could not spawn a child.

    Neither outcome is an answer to compare against — both say the measurement
    itself failed to complete, and the usual cause is the machine being busy
    (both shells run every case, and a full sweep is minutes of load) rather
    than anything wrong with the shell. So retry once: that costs nothing on a
    healthy run, and a case that exceeds its budget — or is refused a process —
    twice in a row is a real hang or a real shell bug rather than a bad moment.
    """
    run = run_case(shell, case)
    if run.timed_out or starved(run):
        run = run_case(shell, case)
    return run


def normalise_stderr(text: str, shell_name: str) -> str:
    """Strip the shell's own name/line prefix from diagnostics.

    Both shells prefix errors with `<name>: line N:`; comparing those verbatim
    would flag every single diagnostic as a divergence purely because the
    programs are called `osh` and `bash`. The *message* is what matters here, so
    drop a leading `<shell>:`/`case.sh:` component. Line numbers are kept — they
    are part of the fidelity being tested.
    """
    out = []
    for line in text.splitlines():
        for prefix in (f"{shell_name}: ", "case.sh: ", "./case.sh: "):
            if line.startswith(prefix):
                line = line[len(prefix):]
                break
        out.append(line)
    return "\n".join(out)


def describe(field: str, a: str, b: str) -> list[str]:
    return [
        f"    {field}:",
        f"      bash: {a!r}",
        f"      osh : {b!r}",
    ]


def compare(case: Case, bash_run: Run, osh_run: Run) -> list[str]:
    """Return a list of human-readable difference lines (empty if identical)."""
    diffs: list[str] = []
    if bash_run.stdout != osh_run.stdout:
        diffs += describe("stdout", bash_run.stdout, osh_run.stdout)
    b_err = normalise_stderr(bash_run.stderr, "bash")
    o_err = normalise_stderr(osh_run.stderr, "osh")
    if b_err != o_err:
        diffs += describe("stderr", b_err, o_err)
    if bash_run.status != osh_run.status:
        diffs += [f"    status: bash={bash_run.status} osh={osh_run.status}"]
    return diffs


def run_report_dir() -> Path:
    """This run's own report directory, created on first use.

    Every run gets a fresh timestamped directory rather than sharing one, so
    that re-running the harness can never destroy the evidence the previous run
    captured — which is the whole point of the reports when the failure being
    chased is intermittent and the obvious next move is "run it again".
    """
    global _RUN_REPORTS
    if _RUN_REPORTS is None:
        stamp = time.strftime("%Y%m%d-%H%M%S")
        _RUN_REPORTS = REPORTS / stamp
        # A second run inside the same second would otherwise write into the
        # first one's directory and mix the two.
        n = 1
        while _RUN_REPORTS.exists():
            _RUN_REPORTS = REPORTS / f"{stamp}-{n}"
            n += 1
    return _RUN_REPORTS


def prune_reports(keep: int = 20) -> None:
    """Drop all but the newest `keep` run directories, so history stays bounded."""
    try:
        dirs = sorted((p for p in REPORTS.iterdir() if p.is_dir()), key=lambda p: p.name)
    except OSError:
        return
    for stale in dirs[: max(0, len(dirs) - keep)]:
        shutil.rmtree(stale, ignore_errors=True)


def write_report(case: Case, bash_run: Run, osh_run: Run, diffs: list[str]) -> Path | None:
    """Leave a failing case's full measurement on disk, and say where.

    Returns None if the report could not be written — a harness that cannot
    save its notes must still report the failure it found.
    """
    path = run_report_dir() / f"{case.name}.txt"
    body = [f"case: {case.path}", "", "differences:", *diffs, ""]
    for label, run in (("bash", bash_run), ("osh", osh_run)):
        body += [
            f"--- {label} status: {run.status}"
            + ("  (timed out)" if run.timed_out else ""),
            f"--- {label} stdout ---",
            run.stdout,
            f"--- {label} stderr ---",
            run.stderr,
        ]
    try:
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text("\n".join(body), encoding="utf-8", errors="replace")
    except OSError:
        return None
    return path


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--osh", help="path to the osh binary (default: newest target/ build)")
    ap.add_argument("--bash", help="path to the reference bash")
    ap.add_argument("-k", "--filter", help="only run cases whose name contains this substring")
    ap.add_argument("-v", "--verbose", action="store_true", help="print each case's output")
    args = ap.parse_args()

    # Cases (and their `# EXPECT-DIFF:` reasons) are UTF-8; the Windows console's
    # default code page would mangle any non-ASCII in them, which reads as a
    # harness bug rather than the cosmetic issue it is.
    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(encoding="utf-8", errors="replace")

    osh = find_shell(args.osh, OSH_CANDIDATES, "osh", newest=True)
    bash = find_shell(args.bash, BASH_CANDIDATES, "bash")
    cases = load_cases(args.filter)
    if not cases:
        sys.exit(f"osh-bash-diff: no cases found in {CORPUS}")

    print(f"osh : {osh}")
    print(f"bash: {bash}")
    print(f"{len(cases)} case(s)\n")

    # Older runs' reports are kept (see `run_report_dir`); only the oldest are
    # retired, so the directory cannot grow without bound.
    prune_reports()

    failures = 0
    waived = 0
    for case in cases:
        bash_run = measure(bash, case)
        osh_run = measure(osh, case)
        diffs = compare(case, bash_run, osh_run)
        if case.expect_diff and diffs:
            waived += 1
            print(f"~ {case.name} (known: {case.expect_diff})")
            continue
        if case.expect_diff and not diffs:
            failures += 1
            print(f"! {case.name}: waived as differing, but now MATCHES bash — remove the")
            print(f"    `# EXPECT-DIFF: {case.expect_diff}` waiver (and close the issue).")
            continue
        if diffs:
            failures += 1
            print(f"X {case.name}")
            # A retry has already been spent, so this one is being reported —
            # but say so, since "the host would not start a process" is a very
            # different thing to chase than a shell divergence.
            if starved(bash_run) or starved(osh_run):
                print("    (a child could not be started, twice — the host is")
                print("     out of commit charge or process slots, not the shell)")
            print("\n".join(diffs))
            report = write_report(case, bash_run, osh_run, diffs)
            print(f"    (full report: {report})" if report else "    (no report written)")
            continue
        print(f". {case.name}")
        if args.verbose:
            print(f"    stdout: {osh_run.stdout!r}")
            print(f"    status: {osh_run.status}")

    matched = len(cases) - failures - waived
    print(f"\n{matched} matched, {waived} waived, {failures} failed")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
