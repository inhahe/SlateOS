#!/usr/bin/env python3
"""Reproducible build recipe for the `fastpy-redirect` SlateOS utility.

This produces `fastpy-redirect.elf`, a native SlateOS (`x86_64-slateos`)
binary compiled by fastpy (AOT Python -> LLVM IR -> native).  It proves
the shell **output-redirection** primitive `cmd > file` end to end from
ring 3 through fastpy's own bindings.

Where `fastpy-capture` and `fastpy-pipeline` redirected a child's stdout
onto a **pipe** (a PIPE kernel handle), this redirects it onto a regular
**file** (a FILE kernel handle) — a *different* kernel handle type, and
so a distinct exercise of the exec fd-table-preservation fix
(`SYS_PROCESS_SET_EXEC_FDS`): the fix must serialise/restore FILE fds
across `execve`, not just PIPE fds, and the child's freshly-opened,
`O_TRUNC`'d output file (offset 0) must survive exec as fd 1 so the
exec'd command's stdout lands in the file.

Primitives combined:

  * `os.open()`   — child opens the output file O_WRONLY|O_CREAT|O_TRUNC
                    (flags 577 = 0o1101), obtaining a raw fd.
  * `os.fork()`   — clone the process (COW).
  * `os.dup2()`   — in the child, point fd 1 (stdout) at the opened file fd,
                    so everything the child writes to stdout lands in the file.
  * `os.execv()`  — replace the child image with the resolved command; the
                    redirected fd 1 (a FILE handle) survives execve.
  * `os.waitpid()`— reap the child and read its exit status.
  * `os.read()`   — in the parent, re-open the output file and read it back
                    to a byte count, cross-checking against the child's exit.

The child becomes `cat <input>` with stdout redirected to `<output>`, so
`cat` copies the input file's bytes into the output file.  `cat` exits
with its byte count; the parent re-opens the output file, counts the
bytes actually written, and asserts they match — proving the FILE-backed
stdout redirect survived exec and captured every byte.  It then exits
with that count, which the kernel self-test
(`self_test_fastpy_slateos_redirect`) asserts equals the input file's
length.

Argv (supplied by the caller):
    argv[1] = command NAME, resolved over PATH (e.g. "cat")
    argv[2] = the input file the command reads
    argv[3] = the output file to create/redirect stdout into

Diagnostic exit codes:
  100 = PATH search matched nothing for the command
  101 = os.execv returned in the child (exec failed)
  102 = os.path.exists failed on the input file
  110 = os.fork() returned -1
  111 = os.waitpid() returned pid -1
  112 = child did not exit normally (os.WIFEXITED false)
  113 = bytes read back from the output file != child's reported exit
        (the FILE-backed stdout redirect lost/duplicated data across exec)
  120 = child's os.open of the output file (O_WRONLY|O_CREAT|O_TRUNC) failed
  121 = parent's os.open of the output file for read-back failed

Run from the fastpy repo root so `compiler` is importable, e.g.:

    PYTHONPATH="D:/visual studio projects/fastpy" \
        python "D:/visual studio projects/os/services/fastpy-redirect/build.py"

The posix sysroot (`libc.a`) must already be built; see
`toolchain/build-sysroot.ps1`.
"""

import ast
from pathlib import Path

from compiler.codegen import CodeGen
from compiler import toolchain

# O_WRONLY(1) | O_CREAT(0o100=64) | O_TRUNC(0o1000=512) = 577; O_RDONLY = 0.
SRC = (
    "import sys\n"
    "import os\n"
    "cmd = sys.argv[1]\n"
    "infile = sys.argv[2]\n"
    "outfile = sys.argv[3]\n"
    "if not os.path.exists(infile):\n"
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
    "if pid < 0:\n"
    "    sys.exit(110)\n"
    "if pid == 0:\n"
    # Child: open the output file, redirect stdout (fd 1) onto it, drop the
    # now-redundant fd, then become the resolved command.  Its stdout writes
    # land in the file; on success execv never returns.
    "    fd = os.open(outfile, 577, 420)\n"
    "    if fd < 0:\n"
    "        sys.exit(120)\n"
    "    os.dup2(fd, 1)\n"
    "    os.close(fd)\n"
    "    os.execv(found, [cmd, infile])\n"
    "    sys.exit(101)\n"
    # Parent: reap the child, then re-open the output file and count what the
    # child actually wrote.
    "rpid, status = os.waitpid(pid, 0)\n"
    "if rpid < 0:\n"
    "    sys.exit(111)\n"
    "if not os.WIFEXITED(status):\n"
    "    sys.exit(112)\n"
    "child_exit = os.WEXITSTATUS(status)\n"
    "rfd = os.open(outfile, 0, 0)\n"
    "if rfd < 0:\n"
    "    sys.exit(121)\n"
    "total = 0\n"
    "while True:\n"
    "    chunk = os.read(rfd, 4096)\n"
    "    if len(chunk) == 0:\n"
    "        break\n"
    "    total = total + len(chunk)\n"
    "os.close(rfd)\n"
    # The bytes now in the output file must equal cat's own exit (cat exits
    # with its byte count) — proving the FILE-backed stdout redirect survived
    # exec and neither dropped nor duplicated data.
    "if total != child_exit:\n"
    "    sys.exit(113)\n"
    "sys.exit(total)\n"
)


def main() -> None:
    ir = CodeGen().generate(ast.parse(SRC))
    out = Path(__file__).resolve().parent
    out.mkdir(parents=True, exist_ok=True)
    obj = toolchain.compile_ir_to_obj(ir, out / "prog.o", target=toolchain.SLATEOS_TARGET)
    exe = toolchain.link_executable([obj], out / "fastpy-redirect.elf", target=toolchain.SLATEOS_TARGET)
    print("OBJ:", obj, obj.stat().st_size)
    print("EXE:", exe, exe.stat().st_size)


if __name__ == "__main__":
    main()
