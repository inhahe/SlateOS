#!/usr/bin/env python3
"""test-proctree.py — tests for `proctree`: launch, cost, and containment.

Containment includes what a tree starts outside its Job Object.

Run directly (no pytest dependency, so it works on a bare Python):

    python scripts/test-proctree.py

These cover the Windows shell-script launch path, which exists because both of
its failure modes are *silent misreports* rather than errors: a `.sh` that
cannot be launched at all, and a bare `bash` that resolves to the WSL shim in
System32 and runs the script in a Linux namespace where none of this project's
tooling exists. A boot test that never ran must never look like a boot test
that passed, so the resolution is worth pinning down.
"""

from __future__ import annotations

import os
import subprocess
import sys
import tempfile
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import proctree  # noqa: E402  (needs the path above)

FAILURES: list[str] = []


def check(name: str, cond: bool, detail: str = "") -> None:
    if cond:
        print(f"  ok   {name}")
    else:
        print(f"  FAIL {name}{': ' + detail if detail else ''}")
        FAILURES.append(name)


def test_passthrough() -> None:
    """Ordinary executables must be handed to Popen untouched."""
    for cmd in (["cargo", "test"], ["taskkill", "/F", "/T"], ["python", "-c", "1"]):
        check(
            f"passthrough {cmd[0]}",
            proctree.resolve_command(cmd) == cmd,
            f"got {proctree.resolve_command(cmd)}",
        )
    # Non-list commands (a raw string for shell=True) are not ours to rewrite.
    check("passthrough str", proctree.resolve_command("echo hi") == "echo hi")
    check("passthrough empty", proctree.resolve_command([]) == [])


def test_shell_script_gets_an_interpreter() -> None:
    """A `.sh` path must gain an absolute bash in front of it."""
    if not proctree.IS_WINDOWS:
        print("  skip (POSIX: the kernel honours `#!` itself)")
        return
    out = proctree.resolve_command(["./scripts/boot-test.sh", "--no-build"])
    check("script: bash prepended", len(out) == 3, f"got {out}")
    check("script: absolute interpreter", os.path.isabs(out[0]), f"got {out[0]}")
    check("script: script path kept", out[1] == "./scripts/boot-test.sh")
    check("script: args preserved", out[2] == "--no-build")


def test_bare_shell_name_is_made_absolute() -> None:
    """`bash`/`sh` must be pinned to a real path, not left for CreateProcess.

    This is the WSL trap: left as the bare name, `CreateProcess` searches
    System32 before PATH and finds the WSL launcher.
    """
    if not proctree.IS_WINDOWS:
        print("  skip (POSIX)")
        return
    for name in ("bash", "sh"):
        out = proctree.resolve_command([name, "-c", "echo hi"])
        check(f"{name}: absolute", os.path.isabs(out[0]), f"got {out[0]}")
        check(f"{name}: not the WSL shim", not proctree._is_wsl_launcher(out[0]))
        check(f"{name}: args preserved", out[1:] == ["-c", "echo hi"], f"got {out}")


def test_explicit_interpreter_path_is_respected() -> None:
    """If the caller already named a specific bash, don't second-guess it."""
    if not proctree.IS_WINDOWS:
        print("  skip (POSIX)")
        return
    explicit = r"C:\msys64\usr\bin\bash.exe"
    out = proctree.resolve_command([explicit, "-c", "echo hi"])
    check("explicit path untouched", out == [explicit, "-c", "echo hi"], f"got {out}")


def test_wsl_launcher_detection() -> None:
    """The shim is identified by directory, not by filename."""
    check("System32 is the shim", proctree._is_wsl_launcher(r"C:\Windows\System32\bash.exe"))
    check("SysWOW64 is the shim", proctree._is_wsl_launcher(r"C:\Windows\SysWOW64\bash.exe"))
    check(
        "Git bash is not the shim",
        not proctree._is_wsl_launcher(r"C:\Program Files\Git\usr\bin\bash.exe"),
    )
    check("missing path is not the shim", not proctree._is_wsl_launcher(r"Z:\nope\bash.exe"))


# --- What a tree starts outside its job (proctree's module docstring) ------
#
# The deterministic stand-in for an App Execution Alias is a tree job that
# lets its members' children leave it (`_let_children_leave`): such a child
# starts outside the job with a member as its parent, which is the shape an
# alias produces. The alias itself is tested too, where the host has one.

# The tree's root: waits for a line on stdin, so the test can change the job
# before anything is started, then starts a sleeper and prints its pid.
_ROOT = ("import subprocess, sys, time\n"
         "sys.stdin.readline()\n"
         "c = subprocess.Popen([sys.executable, '-c', 'import time; time.sleep(120)'])\n"
         "print(c.pid, flush=True)\n"
         "time.sleep(120)\n")

_SYNCHRONIZE = 0x0010_0000
_WAIT_OBJECT_0 = 0


def _let_children_leave(job) -> None:
    """Set `JOB_OBJECT_LIMIT_SILENT_BREAKAWAY_OK` on `job`, so what its members
    start is in no job at all."""
    from ctypes import byref, sizeof
    info = proctree._JOBOBJECT_EXTENDED_LIMIT_INFORMATION()
    k32 = proctree._k32
    if not k32.QueryInformationJobObject(job, proctree.JobObjectExtendedLimitInformation,
                                         byref(info), sizeof(info), None):
        raise OSError("QueryInformationJobObject failed")
    info.BasicLimitInformation.LimitFlags |= 0x1000
    if not k32.SetInformationJobObject(job, proctree.JobObjectExtendedLimitInformation,
                                       byref(info), sizeof(info)):
        raise OSError("SetInformationJobObject failed")


def _hold(pid: int):
    """A handle to `pid` for watching it exit (and, if a test fails, for
    killing what it left): held from the moment its pid is known, so that pid
    cannot pass to another process while the test watches it."""
    return proctree._k32.OpenProcess(
        _SYNCHRONIZE | proctree.PROCESS_TERMINATE | proctree.PROCESS_QUERY_LIMITED_INFORMATION,
        False, pid)


def _exits(handle, seconds: float) -> bool:
    k32 = proctree._k32
    from ctypes import wintypes
    k32.WaitForSingleObject.argtypes = (wintypes.HANDLE, wintypes.DWORD)
    return k32.WaitForSingleObject(handle, int(seconds * 1000)) == _WAIT_OBJECT_0


def _let_go(handle) -> None:
    """Kill what a failed test left running, and close the handle."""
    if handle:
        if not _exits(handle, 0):
            proctree._k32.TerminateProcess(handle, 1)
        proctree._k32.CloseHandle(handle)


def _wait_for(cond, seconds: float) -> bool:
    deadline = time.monotonic() + seconds
    while not cond():
        if time.monotonic() > deadline:
            return False
        time.sleep(0.05)
    return True


def _start_escape(tree) -> int:
    """Make `tree`'s root (running `_ROOT`) start its sleeper outside the job;
    the sleeper's pid."""
    _let_children_leave(tree.job)
    tree.proc.stdin.write(b"go\n")
    tree.proc.stdin.flush()
    return int(tree.proc.stdout.readline())


def test_an_escape_is_adopted_and_dies_with_the_tree() -> None:
    """A process the tree starts outside its job is adopted into one of its own,
    counted, and killed both when the `with` block ends and by `kill`."""
    if not proctree.IS_WINDOWS:
        print("  skip (POSIX: a process group, not a job)")
        return
    for how in ("the block's end", "kill"):
        h = None
        try:
            with proctree.Tree([sys.executable, "-c", _ROOT], stdin=subprocess.PIPE,
                               stdout=subprocess.PIPE) as tree:
                pid = _start_escape(tree)
                h = _hold(pid)
                check(f"{how}: the sleeper really started outside the job",
                      h is not None and not proctree._in_job(h, tree.job))
                check(f"{how}: it is adopted",
                      _wait_for(lambda: pid in tree.adopted, 10), f"adopted={tree.adopted}")
                check(f"{how}: into a job of its own", proctree._in_job(h, None)
                      and not proctree._in_job(h, tree.job))
                acct = tree.accounting()
                check(f"{how}: and its cost is counted with the tree's",
                      acct is not None and acct.processes >= 2, f"{acct!r}")
                if how == "kill":
                    tree.kill()
            check(f"{how}: it dies with the tree", h is not None and _exits(h, 10))
        finally:
            _let_go(h)


def test_an_escape_dies_with_the_runner() -> None:
    """The guarantee that matters most: when the process holding the tree dies,
    an adopted escape dies too, as the rest of the tree does."""
    if not proctree.IS_WINDOWS:
        print("  skip (POSIX)")
        return
    runner = subprocess.Popen([sys.executable, __file__, "--escape-runner"],
                              stdout=subprocess.PIPE)
    h = None
    try:
        line = runner.stdout.readline().decode().split()
        check("runner: its tree's escape was adopted", line[1:] == ["adopted"], f"{line}")
        h = _hold(int(line[0]))
        check("runner: the escape is running", h is not None and not _exits(h, 0))
        runner.kill()
        check("runner: the escape dies with the runner", h is not None and _exits(h, 10))
    finally:
        _let_go(h)
        runner.kill()
        runner.wait()


def _escape_runner() -> int:
    """`--escape-runner`: hold a tree whose sleeper escaped, say its pid once
    it is adopted, and wait to be killed."""
    with proctree.Tree([sys.executable, "-c", _ROOT], stdin=subprocess.PIPE,
                       stdout=subprocess.PIPE) as tree:
        pid = _start_escape(tree)
        adopted = _wait_for(lambda: pid in tree.adopted, 10)
        print(pid, "adopted" if adopted else "not-adopted", flush=True)
        time.sleep(120)
    return 0


def test_a_process_outside_the_tree_is_left_alone() -> None:
    """Adopting a stranger would kill it with the tree. Processes this test
    starts beside the tree, before it and while it runs, must be neither
    adopted nor killed, however many sweeps pass."""
    if not proctree.IS_WINDOWS:
        print("  skip (POSIX)")
        return
    sleeper = [sys.executable, "-c", "import time; time.sleep(60)"]
    before = subprocess.Popen(sleeper)
    during = h = None
    try:
        with proctree.Tree([sys.executable, "-c", _ROOT], stdin=subprocess.PIPE,
                           stdout=subprocess.PIPE) as tree:
            during = subprocess.Popen(sleeper)
            escape = _start_escape(tree)
            h = _hold(escape)
            _wait_for(lambda: escape in tree.adopted, 10)
            time.sleep(3 * proctree.ESCAPE_POLL)
            check("strangers: not adopted",
                  before.pid not in tree.adopted and during.pid not in tree.adopted,
                  f"adopted={tree.adopted}")
        check("strangers: still running after the tree is gone",
              before.poll() is None and during.poll() is None)
    finally:
        _let_go(h)
        for p in (before, during):
            if p is not None:
                p.kill()
                p.wait()


def test_the_root_joins_its_job_before_it_runs() -> None:
    """The root is held suspended until it is in the job. Assignment is made
    slow here, so a root that ran meanwhile would start its child outside."""
    if not proctree.IS_WINDOWS:
        print("  skip (POSIX)")
        return
    assign = proctree.assign_to_job

    def slow_assign(job, proc):
        time.sleep(1.5)
        return assign(job, proc)

    starts_at_once = ("import subprocess, sys, time\n"
                      "c = subprocess.Popen([sys.executable, '-c', 'import time; time.sleep(60)'])\n"
                      "print(c.pid, flush=True)\n"
                      "time.sleep(60)\n")
    proctree.assign_to_job = slow_assign
    h = None
    try:
        with proctree.Tree([sys.executable, "-c", starts_at_once],
                           stdout=subprocess.PIPE) as tree:
            pid = int(tree.proc.stdout.readline())
            h = _hold(pid)
            check("root: its first child was born in the job",
                  h is not None and proctree._in_job(h, tree.job))
            check("root: so it was never an escape", pid not in tree.adopted,
                  f"adopted={tree.adopted}")
    finally:
        proctree.assign_to_job = assign
        _let_go(h)


def _windows_apps_alias() -> str | None:
    """A Python App Execution Alias, where the host has one."""
    for name in ("python.exe", "python3.exe"):
        path = os.path.expandvars(rf"%LOCALAPPDATA%\Microsoft\WindowsApps\{name}")
        if os.path.lexists(path):
            return path
    return None


def test_an_alias_launch_is_contained() -> None:
    """The case this exists for, end to end: run through an App Execution
    Alias by a member of the tree, the app's runtime and what it starts die
    with the tree. Needs an alias to run, which a stock host may not have -- a
    named skip there, not a quiet pass."""
    alias = _windows_apps_alias() if proctree.IS_WINDOWS else None
    if alias is None:
        print("  SKIP no Python App Execution Alias on this host")
        return
    # As the root, the alias joins the job like any child, and its status
    # comes back -- which also shows a suspended start does not upset it.
    res = proctree.run_captured([alias, "-c", "import sys; sys.exit(3)"], timeout=60)
    check("alias as the root: runs, and its status is passed through",
          res.returncode == 3, f"rc={res.returncode} stderr={res.stderr[-300:]!r}")
    handles = []
    with tempfile.TemporaryDirectory() as td:
        # A file, not the tree's stdout: whether a brokered process's output
        # reaches a pipe is up to the package's launcher.
        pidfile = Path(td) / "pids"
        runtime = ("import os, pathlib, subprocess, sys, time\n"
                   "c = subprocess.Popen([sys.executable, '-c', 'import time; time.sleep(120)'])\n"
                   f"pathlib.Path({str(pidfile)!r}).write_text(f'{{os.getpid()}} {{c.pid}}')\n"
                   "time.sleep(120)\n")
        member = (f"import subprocess, time\n"
                  f"subprocess.Popen([{alias!r}, '-c', {runtime!r}])\n"
                  f"time.sleep(120)\n")
        try:
            with proctree.Tree([sys.executable, "-c", member]) as tree:
                started = _wait_for(lambda: len(pidfile.read_text().split()) == 2
                                    if pidfile.exists() else False, 30)
                check("alias by a member: its runtime ran", started)
                if not started:
                    return
                pids = [int(p) for p in pidfile.read_text().split()]
                handles = [_hold(p) for p in pids]
                check("alias by a member: its runtime is adopted",
                      _wait_for(lambda: pids[0] in tree.adopted, 10), f"adopted={tree.adopted}")
            check("alias by a member: the runtime and its child die with the tree",
                  all(h is not None and _exits(h, 10) for h in handles))
        finally:
            for h in handles:
                _let_go(h)


def test_shebang_detection() -> None:
    """Extensionless scripts are recognised from their `#!` line."""
    with tempfile.TemporaryDirectory() as td:
        script = Path(td) / "noext"
        script.write_bytes(b"#!/usr/bin/env bash\necho hi\n")
        check("shebang detected", proctree._is_shell_script(str(script)))

        binary = Path(td) / "prog.exe"
        binary.write_bytes(b"MZ\x90\x00" + b"\x00" * 60)
        check("binary not a script", not proctree._is_shell_script(str(binary)))

        python = Path(td) / "tool"
        python.write_bytes(b"#!/usr/bin/env python3\nprint(1)\n")
        check("python shebang not a shell script", not proctree._is_shell_script(str(python)))

        # The interpreter is matched by basename, not by substring: these two
        # both contain "sh" but are not shell scripts, and interposing bash on
        # them would fail in a thoroughly confusing way.
        shpath = Path(td) / "inshdir"
        shpath.write_bytes(b"#!/home/shared/bin/python3\nprint(1)\n")
        check("'sh' inside a path is not a shell", not proctree._is_shell_script(str(shpath)))

        fish = Path(td) / "fishy"
        fish.write_bytes(b"#!/usr/bin/fish\necho hi\n")
        check("fish is not a POSIX shell", not proctree._is_shell_script(str(fish)))

        # ...while the real ones, direct or via env, are.
        for interp, label in ((b"/bin/sh", "sh"), (b"/bin/dash", "dash"), (b"/usr/bin/env zsh", "env zsh")):
            f = Path(td) / f"s_{label.replace(' ', '_')}"
            f.write_bytes(b"#!" + interp + b"\necho hi\n")
            check(f"{label} shebang detected", proctree._is_shell_script(str(f)))

        check("missing file not a script", not proctree._is_shell_script(str(Path(td) / "gone")))


def test_shell_is_usable() -> None:
    """The resolved shell must actually be an MSYS-family bash.

    A shell that runs but cannot see `cygpath` or the project's Windows-path
    tooling is the WSL failure wearing a different hat, so assert on a
    capability rather than on the path string.
    """
    shell = proctree.find_unix_shell() if proctree.IS_WINDOWS else "/bin/sh"
    if shell is None:
        check("a unix shell exists", False, "find_unix_shell() returned None")
        return
    res = proctree.run_captured([shell, "-c", "echo MARKER-$((6*7))"], timeout=30)
    out = res.stdout.decode("utf-8", "replace")
    check("shell runs", "MARKER-42" in out, f"stdout={out!r} rc={res.returncode}")
    check("shell did not time out", not res.timed_out)


def test_tree_runs_a_script_end_to_end() -> None:
    """The whole point: `Tree(["...sh"])` must run, and report the real status."""
    with tempfile.TemporaryDirectory() as td:
        script = Path(td) / "probe.sh"
        script.write_bytes(b'#!/usr/bin/env bash\necho "ARG=$1"\nexit 7\n')
        res = proctree.run_captured([str(script), "hello"], timeout=60)
        out = res.stdout.decode("utf-8", "replace")
        check("script ran", "ARG=hello" in out, f"stdout={out!r}")
        # The exit code must be the *script's*, not a launch failure — the bug
        # this guards against reported success for a script that never started.
        check("script exit code passed through", res.returncode == 7, f"rc={res.returncode}")


def test_tree_accounting_counts_short_lived_processes() -> None:
    """`Tree.accounting()` is read from the Job Object, so it counts every
    process the tree ran, including ones that exited in milliseconds -- the
    reason it exists (see `job_accounting`). A shell that runs a few external
    commands and exits must report at least that many processes, and some CPU.
    After the job closes there is nothing to ask, and it says so with None
    rather than a zero that would read as "cost nothing"."""
    if not proctree.IS_WINDOWS:
        with proctree.Tree([sys.executable, "-c", "pass"]) as tree:
            tree.proc.wait(timeout=60)
            check("POSIX: no job, so no accounting", tree.accounting() is None,
                  f"got {tree.accounting()!r}")
        return
    with tempfile.TemporaryDirectory() as td:
        script = Path(td) / "spawns.sh"
        # Five external commands, each its own process -- two, under MSYS,
        # where a fork and the exec it becomes are separate Windows processes.
        script.write_bytes(b"#!/usr/bin/env bash\n"
                           b"for i in 1 2 3 4 5; do /usr/bin/true; done\n"
                           b"exit 0\n")
        with proctree.Tree([str(script)]) as tree:
            rc = tree.proc.wait(timeout=120)
            acct = tree.accounting()
        check("accounting: the script ran", rc == 0, f"rc={rc}")
        check("accounting: available while the job is open", acct is not None)
        if acct is not None:
            check("accounting: every process counted, exited ones included",
                  acct.processes >= 6, f"processes={acct.processes}")
            check("accounting: some CPU was charged", acct.cpu_s > 0,
                  f"cpu_s={acct.cpu_s}")
            check("accounting: the summary names the process count",
                  f"across {acct.processes} process(es)" in acct.summary(),
                  acct.summary())
        check("accounting: None once the job is closed", tree.accounting() is None,
              f"got {tree.accounting()!r}")


def main() -> int:
    tests = [
        test_passthrough,
        test_shell_script_gets_an_interpreter,
        test_bare_shell_name_is_made_absolute,
        test_explicit_interpreter_path_is_respected,
        test_wsl_launcher_detection,
        test_shebang_detection,
        test_shell_is_usable,
        test_tree_runs_a_script_end_to_end,
        test_tree_accounting_counts_short_lived_processes,
        test_the_root_joins_its_job_before_it_runs,
        test_an_escape_is_adopted_and_dies_with_the_tree,
        test_an_escape_dies_with_the_runner,
        test_a_process_outside_the_tree_is_left_alone,
        test_an_alias_launch_is_contained,
    ]
    for t in tests:
        print(f"{t.__name__}:")
        t()
    print()
    if FAILURES:
        print(f"FAILED ({len(FAILURES)}): {', '.join(FAILURES)}")
        return 1
    print("all proctree tests passed")
    return 0


if __name__ == "__main__":
    if sys.argv[1:] == ["--escape-runner"]:
        sys.exit(_escape_runner())
    sys.exit(main())
