#!/usr/bin/env python3
"""Reproducible build recipe for the `fastpy-forkexec` SlateOS utility.

This produces `fastpy-forkexec.elf`, a native SlateOS (`x86_64-slateos`)
binary compiled by fastpy (AOT Python -> LLVM IR -> native).  It is the first
fastpy component to exercise the **classic fork/exec/wait trinity** entirely
from Python bindings: `os.fork()` to clone the process, `os.execv()` in the
child to become another program, and `os.waitpid()` in the parent to reap the
child and read its exit status.

Where `fastpy-run` *replaced* its own image with the resolved command (execv
in-place, single process), `fastpy-forkexec` keeps running: it **forks a
child**, has the child `os.execv` the resolved command, and the parent
**blocks in `os.waitpid`** until the child exits, then propagates the child's
exit code as its own.  This is exactly how a shell / `init` spawns a program
without terminating itself.

The chain lowers bridge-free on SlateOS:

  * `os.fork()`   -> posix `fork()`   -> `SYS_PROCESS_FORK` (COW address-space
                     clone; child resumes with RAX forced to 0, parent gets the
                     child PID).
  * `os.execv()`  -> posix `execv()`  -> `SYS_EXECVE` (child's image replaced).
  * `os.waitpid()`-> posix `waitpid()`-> the kernel's wait/reap path (parent
                     blocks until the child is a zombie, then collects its
                     encoded status).
  * `os.WIFEXITED`/`os.WEXITSTATUS` decode that status.

The kernel self-test (`self_test_fastpy_slateos_forkexec`) stages a known file
in `/tmp`, spawns `fastpy-forkexec cat /tmp/...`, and asserts that (a) `cat`'s
output appears on serial (proving the forked child really became `cat`) and
(b) the fastpy process exits with `cat`'s byte count (proving the parent
reaped the child and read the correct exit status) — the fork/exec/wait
handoff proven end to end from ring 3 through fastpy's own bindings.

Run from the fastpy repo root so `compiler` is importable, e.g.:

    PYTHONPATH="D:/visual studio projects/fastpy" \
        python "D:/visual studio projects/os/services/fastpy-forkexec/build.py"

The posix sysroot (`libc.a`) must already be built; see
`toolchain/build-sysroot.ps1`.
"""

import ast
from pathlib import Path

from compiler.codegen import CodeGen
from compiler import toolchain

# fork/exec/wait runner.  argv[1] = command name, argv[2] = the single argument
# to hand it.  A bogus first PATH entry proves the search skips directories that
# don't hold the command before finding it in /mnt/bin.
#
# Diagnostic exit codes (so the kernel harness can pinpoint a failure):
#   100 = PATH search matched nothing (os.path.exists never true for cmd)
#   101 = os.execv returned in the child, i.e. exec of the resolved path FAILED
#   102 = os.path.exists failed even on the known-good /tmp target (stat broken)
#   110 = os.fork() returned -1 (fork itself failed)
#   111 = os.waitpid() returned pid -1 (reap failed)
#   112 = child did not exit normally (os.WIFEXITED was false)
# On success the parent exits with os.WEXITSTATUS(status) — the child `cat`'s
# own exit code (its byte count), which the harness asserts.
SRC = (
    "import sys\n"
    "import os\n"
    "cmd = sys.argv[1]\n"
    "target = sys.argv[2]\n"
    "if not os.path.exists(target):\n"
    "    sys.exit(102)\n"
    "PATH = ['/nonexistent/bin', '/mnt/bin']\n"
    "found = ''\n"
    "for d in PATH:\n"
    "    p = d + '/' + cmd\n"
    "    if os.path.exists(p):\n"
    "        found = p\n"
    "if found == '':\n"
    "    sys.exit(100)\n"
    "pid = os.fork()\n"
    "if pid == 0:\n"
    # Child: replace our image with the resolved command.  On success execv
    # never returns (the child becomes `cmd`); only a failed exec falls through.
    "    os.execv(found, [cmd, target])\n"
    "    sys.exit(101)\n"
    "if pid < 0:\n"
    "    sys.exit(110)\n"
    # Parent: block until the child exits, then propagate its status.
    "rpid, status = os.waitpid(pid, 0)\n"
    "if rpid < 0:\n"
    "    sys.exit(111)\n"
    "if not os.WIFEXITED(status):\n"
    "    sys.exit(112)\n"
    "sys.exit(os.WEXITSTATUS(status))\n"
)


def main() -> None:
    ir = CodeGen().generate(ast.parse(SRC))
    out = Path(__file__).resolve().parent
    out.mkdir(parents=True, exist_ok=True)
    obj = toolchain.compile_ir_to_obj(ir, out / "prog.o", target=toolchain.SLATEOS_TARGET)
    exe = toolchain.link_executable([obj], out / "fastpy-forkexec.elf", target=toolchain.SLATEOS_TARGET)
    print("OBJ:", obj, obj.stat().st_size)
    print("EXE:", exe, exe.stat().st_size)


if __name__ == "__main__":
    main()
