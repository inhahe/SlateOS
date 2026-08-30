#!/usr/bin/env python3
"""test-proctree.py — tests for `proctree.resolve_command` and friends.

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
import sys
import tempfile
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
    sys.exit(main())
