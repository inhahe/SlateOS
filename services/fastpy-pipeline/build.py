#!/usr/bin/env python3
"""Reproducible build recipe for the `fastpy-pipeline` SlateOS utility.

This produces `fastpy-pipeline.elf`, a native SlateOS (`x86_64-slateos`)
binary compiled by fastpy (AOT Python -> LLVM IR -> native).  It runs a
genuine two-stage shell pipeline `cmd1 | cmd2` end to end from ring 3
through fastpy's own bindings — the machinery behind a `cat file | wc`
style command line.

Where `fastpy-capture` proved the *producer* half of a pipe (a child
`dup2`s its stdout onto the pipe's write end, `execv`s a command, and the
parent drains the pipe itself), this utility proves BOTH halves at once:
a real downstream command whose **stdin** has been pointed at the pipe's
read end.  That consumer-side redirect (`dup2(pipe_r, 0)` surviving
`execv`) is the direction `fastpy-capture` never exercised.

Data flow:

    producer (cat target) --stdout--> [pipe] --stdin--> consumer (countin)

Primitives combined:

  * `os.pipe()`    — one kernel pipe shared by the two children.
  * `os.fork()`    — twice: one child per pipeline stage.
  * `os.dup2()`    — producer points fd 1 (stdout) at the pipe write end;
                     consumer points fd 0 (stdin) at the pipe read end.
  * `os.execv()`   — each child becomes its command; both redirects survive
                     execve (the kernel reuses the child's fd/handle table).
  * `os.waitpid()` — the parent reaps both children and reads their exits.

The parent closes BOTH pipe ends after forking (so the consumer sees EOF
once the producer exits), reaps both children, and cross-checks that the
producer's byte count (cat exits with its byte count) equals the
consumer's byte count (countin exits with the bytes it read off stdin) —
proving the pipe neither dropped nor duplicated data across the two exec'd
processes.  It then exits with that shared count, which the kernel
self-test (`self_test_fastpy_slateos_pipeline`) asserts equals the staged
file's length.

Argv (supplied by the caller):
    argv[1] = producer command NAME, resolved over PATH (e.g. "cat")
    argv[2] = consumer ELF PATH, exec'd directly (e.g. the countin fixture)
    argv[3] = the target file the producer reads

Diagnostic exit codes:
  100 = PATH search matched nothing for the producer command
  101 = os.execv returned in the producer child (exec failed)
  102 = os.path.exists failed on the target file
  103 = os.path.exists failed on the consumer ELF path
  110 = first os.fork() (producer) returned -1
  111 = an os.waitpid() returned pid -1
  112 = producer did not exit normally (os.WIFEXITED false)
  113 = producer byte count != consumer byte count (pipe lost/dup'd data)
  114 = second os.fork() (consumer) returned -1
  115 = os.execv returned in the consumer child (exec failed)
  116 = consumer did not exit normally (os.WIFEXITED false)

Run with fastpy on PYTHONPATH so `compiler` is importable, from the root of
the worktree you are actually working in. There are four checkouts of this
repo, and naming one of them in a command is how a lane ends up building
another lane's artifact -- see `scripts/lib/worktree.sh`:

    PYTHONPATH="D:/visual studio projects/fastpy" \
        python services/fastpy-pipeline/build.py

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
    "consumer = sys.argv[2]\n"
    "target = sys.argv[3]\n"
    "if not os.path.exists(target):\n"
    "    sys.exit(102)\n"
    "if not os.path.exists(consumer):\n"
    "    sys.exit(103)\n"
    "PATH = ['/nonexistent/bin', '/mnt/bin']\n"
    "found = ''\n"
    "for d in PATH:\n"
    "    p = d + '/' + cmd\n"
    "    if os.path.exists(p):\n"
    "        found = p\n"
    "if found == '':\n"
    "    sys.exit(100)\n"
    "r, w = os.pipe()\n"
    "p1 = os.fork()\n"
    "if p1 < 0:\n"
    "    sys.exit(110)\n"
    "if p1 == 0:\n"
    # Producer: redirect stdout (fd 1) onto the pipe write end, drop the now-
    # redundant pipe fds, then become the resolved command.  Its stdout writes
    # flow into the pipe; on success execv never returns.
    "    os.dup2(w, 1)\n"
    "    os.close(r)\n"
    "    os.close(w)\n"
    "    os.execv(found, [cmd, target])\n"
    "    sys.exit(101)\n"
    "p2 = os.fork()\n"
    "if p2 < 0:\n"
    "    sys.exit(114)\n"
    "if p2 == 0:\n"
    # Consumer: redirect stdin (fd 0) onto the pipe read end, drop the pipe fds,
    # then become the consumer command.  Its os.read(0, ...) draws from the pipe.
    "    os.dup2(r, 0)\n"
    "    os.close(r)\n"
    "    os.close(w)\n"
    "    os.execv(consumer, ['countin'])\n"
    "    sys.exit(115)\n"
    # Parent: close BOTH ends so the consumer sees EOF once the producer exits
    # (the only remaining write-end reference is the producer's fd 1).
    "os.close(r)\n"
    "os.close(w)\n"
    "rp1, st1 = os.waitpid(p1, 0)\n"
    "rp2, st2 = os.waitpid(p2, 0)\n"
    "if rp1 < 0:\n"
    "    sys.exit(111)\n"
    "if rp2 < 0:\n"
    "    sys.exit(111)\n"
    "if not os.WIFEXITED(st1):\n"
    "    sys.exit(112)\n"
    "if not os.WIFEXITED(st2):\n"
    "    sys.exit(116)\n"
    "e1 = os.WEXITSTATUS(st1)\n"
    "e2 = os.WEXITSTATUS(st2)\n"
    # cat exits with the bytes it wrote; countin exits with the bytes it read.
    # Equal counts prove the pipe carried every byte exactly once between two
    # independently exec'd processes.
    "if e1 != e2:\n"
    "    sys.exit(113)\n"
    "sys.exit(e2)\n"
)


def main() -> None:
    ir = CodeGen().generate(ast.parse(SRC))
    out = Path(__file__).resolve().parent
    out.mkdir(parents=True, exist_ok=True)
    obj = toolchain.compile_ir_to_obj(ir, out / "prog.o", target=toolchain.SLATEOS_TARGET)
    exe = toolchain.link_executable([obj], out / "fastpy-pipeline.elf", target=toolchain.SLATEOS_TARGET)
    print("OBJ:", obj, obj.stat().st_size)
    print("EXE:", exe, exe.stat().st_size)


if __name__ == "__main__":
    main()
