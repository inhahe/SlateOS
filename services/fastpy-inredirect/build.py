#!/usr/bin/env python3
"""Reproducible build recipe for the `fastpy-inredirect` SlateOS utility.

This produces `fastpy-inredirect.elf`, a native SlateOS (`x86_64-slateos`)
binary compiled by fastpy (AOT Python -> LLVM IR -> native).  It proves
the shell **input-redirection** primitive `cmd < file` end to end from
ring 3 through fastpy's own bindings.

Where `fastpy-redirect` redirected a child's stdout onto a regular
**file** (a FILE kernel handle) as fd 1, this redirects a regular
**file** onto the child's stdin (fd 0) — the *input* counterpart, and a
distinct exercise of the exec fd-table-preservation fix
(`SYS_PROCESS_SET_EXEC_FDS`): the fix must serialise/restore a *readable*
FILE fd across `execve` and reinstate it as fd 0, so the exec'd command
reads the file's bytes from stdin.  This is the FILE-on-stdin case; the
`fastpy-pipeline` consumer proved PIPE-on-stdin and `fastpy-redirect`
proved FILE-on-stdout, so together they cover both handle types on both
standard streams across exec.

Primitives combined:

  * `os.open()`   — child opens the input file O_RDONLY (flags 0),
                    obtaining a raw fd.
  * `os.fork()`   — clone the process (COW).
  * `os.dup2()`   — in the child, point fd 0 (stdin) at the opened file fd,
                    so everything the child reads from stdin comes from the
                    file.
  * `os.execv()`  — replace the child image with the consumer; the
                    redirected fd 0 (a readable FILE handle) survives execve.
  * `os.waitpid()`— reap the child and read its exit status.
  * `os.read()`   — in the parent, re-open the input file and read it back
                    to a byte count, cross-checking against the child's exit.

The child becomes the consumer (`fastpy-countin`, which reads stdin to EOF
and exits with the total byte count) with stdin redirected from the input
file.  If the readable FILE fd survived exec as fd 0, the consumer reads
the file's bytes from stdin and exits with the file's length.  The parent
re-opens the input file, counts its bytes, and asserts they match the
child's exit — proving the FILE-backed stdin redirect survived exec and
delivered every byte to the child's stdin.  It then exits with that count,
which the kernel self-test (`self_test_fastpy_slateos_inredirect`) asserts
equals the input file's length.

Argv (supplied by the caller):
    argv[1] = consumer PROGRAM PATH (absolute), a program that reads stdin
              to EOF and exits with the byte count (e.g. fastpy-countin)
    argv[2] = the input file to redirect onto the consumer's stdin

Diagnostic exit codes:
  101 = os.execv returned in the child (exec failed)
  102 = os.path.exists failed on the input file
  103 = os.path.exists failed on the consumer program
  110 = os.fork() returned -1
  111 = os.waitpid() returned pid -1
  112 = child did not exit normally (os.WIFEXITED false)
  113 = bytes read back from the input file != child's reported exit
        (the FILE-backed stdin redirect lost/duplicated data across exec)
  120 = child's os.open of the input file (O_RDONLY) failed
  121 = parent's os.open of the input file for read-back failed

Run with fastpy on PYTHONPATH so `compiler` is importable, from the root of
the worktree you are actually working in. There are four checkouts of this
repo, and naming one of them in a command is how a lane ends up building
another lane's artifact -- see `scripts/lib/worktree.sh`:

    PYTHONPATH="D:/visual studio projects/fastpy" \
        python services/fastpy-inredirect/build.py

The posix sysroot (`libc.a`) must already be built; see
`toolchain/build-sysroot.ps1`.
"""

import ast
from pathlib import Path

from compiler.codegen import CodeGen
from compiler import toolchain

# O_RDONLY = 0.
SRC = (
    "import sys\n"
    "import os\n"
    "consumer = sys.argv[1]\n"
    "infile = sys.argv[2]\n"
    "if not os.path.exists(infile):\n"
    "    sys.exit(102)\n"
    "if not os.path.exists(consumer):\n"
    "    sys.exit(103)\n"
    "pid = os.fork()\n"
    "if pid < 0:\n"
    "    sys.exit(110)\n"
    "if pid == 0:\n"
    # Child: open the input file O_RDONLY, redirect stdin (fd 0) onto it, drop
    # the now-redundant fd, then become the consumer.  Everything it reads from
    # stdin comes from the file; on success execv never returns.
    "    fd = os.open(infile, 0, 0)\n"
    "    if fd < 0:\n"
    "        sys.exit(120)\n"
    "    os.dup2(fd, 0)\n"
    "    os.close(fd)\n"
    "    os.execv(consumer, ['countin'])\n"
    "    sys.exit(101)\n"
    # Parent: reap the child, then re-open the input file and count its bytes.
    "rpid, status = os.waitpid(pid, 0)\n"
    "if rpid < 0:\n"
    "    sys.exit(111)\n"
    "if not os.WIFEXITED(status):\n"
    "    sys.exit(112)\n"
    "child_exit = os.WEXITSTATUS(status)\n"
    "rfd = os.open(infile, 0, 0)\n"
    "if rfd < 0:\n"
    "    sys.exit(121)\n"
    "total = 0\n"
    "while True:\n"
    "    chunk = os.read(rfd, 4096)\n"
    "    if len(chunk) == 0:\n"
    "        break\n"
    "    total = total + len(chunk)\n"
    "os.close(rfd)\n"
    # The bytes the consumer read from its redirected stdin (its exit) must
    # equal the file's length — proving the FILE-backed stdin redirect survived
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
    exe = toolchain.link_executable([obj], out / "fastpy-inredirect.elf", target=toolchain.SLATEOS_TARGET)
    print("OBJ:", obj, obj.stat().st_size)
    print("EXE:", exe, exe.stat().st_size)


if __name__ == "__main__":
    main()
