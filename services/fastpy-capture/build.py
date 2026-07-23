#!/usr/bin/env python3
"""Reproducible build recipe for the `fastpy-capture` SlateOS utility.

This produces `fastpy-capture.elf`, a native SlateOS (`x86_64-slateos`) binary
compiled by fastpy (AOT Python -> LLVM IR -> native).  It is the first fastpy
component to **capture a child process's stdout through a pipe** — the last
core shell primitive, and the machinery behind command substitution `$(...)`
and one stage of a `cmd1 | cmd2` pipeline.

It combines every process/IO primitive fastpy now has:

  * `os.pipe()`    — create a kernel pipe (SYS_PIPE_CREATE); [read_fd, write_fd].
  * `os.fork()`    — clone the process (SYS_PROCESS_FORK, COW).
  * `os.dup2()`    — in the child, point fd 1 (stdout) at the pipe's write end,
                     so everything the child writes to stdout flows into the
                     pipe instead of the console.
  * `os.execv()`   — replace the child image with the resolved command; the
                     redirected fd 1 survives execve (the kernel reuses the
                     child's fd/handle table), so the command's stdout is piped.
  * `os.read()`    — in the parent, drain the pipe (SYS_PIPE_READ) until EOF.
  * `os.waitpid()` — reap the child and read its exit status.

The parent closes its copy of the write end (so the read side sees EOF once the
child is done), reads the pipe to a byte count, reaps the child, and cross-
checks that the number of bytes it captured equals the child's own exit code
(fastpy `cat` exits with its byte count).  It then exits with that captured
count, which the kernel self-test (`self_test_fastpy_slateos_capture`) asserts
equals the staged file's length — proving fork + dup2(stdout->pipe) + exec +
pipe-drain worked end to end from ring 3 through fastpy's own bindings.

Diagnostic exit codes:
  100 = PATH search matched nothing
  101 = os.execv returned in the child (exec failed)
  102 = os.path.exists failed on the known /tmp target (stat broken)
  110 = os.fork() returned -1
  111 = os.waitpid() returned pid -1
  112 = child did not exit normally (os.WIFEXITED false)
  113 = captured byte count != child's reported exit (pipe/dup2 lost data)

Run from the fastpy repo root so `compiler` is importable, e.g.:

    PYTHONPATH="D:/visual studio projects/fastpy" \
        python "D:/visual studio projects/os/services/fastpy-capture/build.py"

The posix sysroot (`libc.a`) must already be built; see
`toolchain/build-sysroot.ps1`.
"""

import ast
from pathlib import Path

from compiler.codegen import CodeGen
from compiler import toolchain

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
    "r, w = os.pipe()\n"
    "pid = os.fork()\n"
    "if pid < 0:\n"
    "    sys.exit(110)\n"
    "if pid == 0:\n"
    # Child: redirect stdout (fd 1) to the pipe's write end, drop the now-
    # redundant pipe fds, then become the resolved command.  Its stdout writes
    # flow into the pipe; on success execv never returns.
    "    os.dup2(w, 1)\n"
    "    os.close(r)\n"
    "    os.close(w)\n"
    "    os.execv(found, [cmd, target])\n"
    "    sys.exit(101)\n"
    # Parent: close the write end so our read sees EOF when the child finishes,
    # then drain the pipe counting bytes.
    "os.close(w)\n"
    "total = 0\n"
    "while True:\n"
    "    chunk = os.read(r, 4096)\n"
    "    if len(chunk) == 0:\n"
    "        break\n"
    "    total = total + len(chunk)\n"
    "os.close(r)\n"
    "rpid, status = os.waitpid(pid, 0)\n"
    "if rpid < 0:\n"
    "    sys.exit(111)\n"
    "if not os.WIFEXITED(status):\n"
    "    sys.exit(112)\n"
    # The bytes we captured off the pipe must equal the child cat's own exit
    # (cat exits with its byte count) — a self-consistency check that the pipe
    # neither dropped nor duplicated data.
    "if os.WEXITSTATUS(status) != total:\n"
    "    sys.exit(113)\n"
    "sys.exit(total)\n"
)


def main() -> None:
    ir = CodeGen().generate(ast.parse(SRC))
    out = Path(__file__).resolve().parent
    out.mkdir(parents=True, exist_ok=True)
    obj = toolchain.compile_ir_to_obj(ir, out / "prog.o", target=toolchain.SLATEOS_TARGET)
    exe = toolchain.link_executable([obj], out / "fastpy-capture.elf", target=toolchain.SLATEOS_TARGET)
    print("OBJ:", obj, obj.stat().st_size)
    print("EXE:", exe, exe.stat().st_size)


if __name__ == "__main__":
    main()
