#!/usr/bin/env python3
"""Reproducible build recipe for the `fastpy-run` SlateOS utility.

This produces `fastpy-run.elf`, a native SlateOS (`x86_64-slateos`) binary
compiled by fastpy (AOT Python -> LLVM IR -> native).  It is the first fastpy
component to **resolve an installed command by name over a PATH and hand the
process off to it via `os.execv`** — the core primitive a shell / `init` uses
to run a program.

Where every prior fastpy utility performed its whole job inside its own image,
`fastpy-run` does almost nothing itself: it takes a command name (`argv[1]`)
and a file argument (`argv[2]`), walks a PATH list (`["/nonexistent/bin",
"/mnt/bin"]`, where the second dir is the rootfs `/bin` mount that holds the
promoted fastpy coreutils), finds the first directory that actually contains
the command (`os.path.exists`), and **replaces its own process image** with
that command via `os.execv(path, [cmd, arg])`.  On success `execv` never
returns — the process becomes e.g. `/bin/cat`, which reads the file and exits
with its own status; on failure (command not found or exec error) the runner
falls through and exits 127 (the shell "command not found" convention).

This exercises a brand-new on-target path: userspace-initiated program
execution.  `os.execv` lowers bridge-free to the SlateOS posix `execv()` ->
`SYS_EXECVE`, which reuses the caller's PID (so the resolved command inherits
the runner's capabilities and open handles — including the File capability and
stdout console handle the self-test grants) and rewrites the trap frame to
enter the new ELF.  The kernel self-test (`self_test_fastpy_slateos_run`)
stages a known file in `/tmp`, spawns `fastpy-run cat /tmp/...`, and asserts
that `cat`'s output appears on serial and the process exits with `cat`'s byte
count — proving the resolve+exec handoff end to end from ring 3.

Run with fastpy on PYTHONPATH so `compiler` is importable, from the root of
the worktree you are actually working in. There are four checkouts of this
repo, and naming one of them in a command is how a lane ends up building
another lane's artifact -- see `scripts/lib/worktree.sh`:

    PYTHONPATH="D:/visual studio projects/fastpy" \
        python services/fastpy-run/build.py

The posix sysroot (`libc.a`) must already be built; see
`toolchain/build-sysroot.ps1`.
"""

import ast
from pathlib import Path

from compiler.codegen import CodeGen
from compiler import toolchain

# Minimal PATH-resolving exec runner.  argv[1] = command name, argv[2] = the
# single argument to hand it.  A bogus first PATH entry proves the search skips
# directories that don't hold the command before finding it in /mnt/bin.
SRC = (
    "import sys\n"
    "import os\n"
    "cmd = sys.argv[1]\n"
    "target = sys.argv[2]\n"
    # Diagnostic probe: `target` is a known-existing /tmp (memfs) file the
    # harness staged.  os.path.exists on it MUST succeed; if it doesn't, the
    # stat mechanism itself is broken in this ELF (codegen / posix stat) and
    # the /mnt PATH probe below is a red herring.  Exit 102 isolates that.
    "if not os.path.exists(target):\n"
    "    sys.exit(102)\n"
    "PATH = ['/nonexistent/bin', '/mnt/bin']\n"
    "found = ''\n"
    "for d in PATH:\n"
    "    p = d + '/' + cmd\n"
    "    if os.path.exists(p):\n"
    "        found = p\n"
    # Diagnostic exit codes so the kernel harness can pinpoint a failure:
    #   100 = PATH search matched nothing (os.path.exists never true for cmd)
    #   101 = os.execv returned, i.e. exec of the resolved path FAILED
    #   102 = os.path.exists failed even on the known-good /tmp target
    # On success execv never returns (the image becomes `cmd`), so none of
    # these fires and the process exits with the handed-off command's status.
    "if found == '':\n"
    "    sys.exit(100)\n"
    "os.execv(found, [cmd, target])\n"
    "sys.exit(101)\n"
)


def main() -> None:
    ir = CodeGen().generate(ast.parse(SRC))
    out = Path(__file__).resolve().parent
    out.mkdir(parents=True, exist_ok=True)
    obj = toolchain.compile_ir_to_obj(ir, out / "prog.o", target=toolchain.SLATEOS_TARGET)
    exe = toolchain.link_executable([obj], out / "fastpy-run.elf", target=toolchain.SLATEOS_TARGET)
    print("OBJ:", obj, obj.stat().st_size)
    print("EXE:", exe, exe.stat().st_size)


if __name__ == "__main__":
    main()
