#!/usr/bin/env python3
"""Reproducible build recipe for the `fastpy-pipe` SlateOS utility.

This produces `fastpy-pipe.elf`, a native SlateOS (`x86_64-slateos`) binary
compiled by fastpy (AOT Python -> LLVM IR -> native): a tool that creates a
kernel **pipe** and round-trips a message through it, exercising the pipe
subsystem — a genuinely distinct kernel path from the file/process syscalls the
other fastpy tools use.

`os.pipe()` lowers to native `fastpy_os_pipe` -> posix `pipe()` -> kernel
`SYS_PIPE_CREATE` (allocating a kernel pipe + two fds).  The two raw fds are
then driven by native raw-fd I/O:
  * `os.write(w, msg)` -> native `fastpy_os_write` -> posix `write()` -> the
    pipe's kernel buffer via `SYS_PIPE_WRITE` (posix dispatches by fd kind),
  * `os.read(r, n)`    -> native `fastpy_os_read`  -> posix `read()`  -> the
    pipe's kernel buffer via `SYS_PIPE_READ`,
  * `os.close(fd)`     -> native `fastpy_os_close` -> posix `close()` ->
    `SYS_PIPE_CLOSE`.

This is the first fastpy tool to exercise raw integer fds (as opposed to the
high-level `open()`/file-object I/O every other tool uses) and the first to
touch the kernel pipe subsystem at all.

The tool writes the *round-tripped* message to `/tmp/fastpy-pipe.out` (via a
native `open('w')`/`write`) so the kernel self-test can read it back, and exits
with a code encoding a self-consistency check:

    exit 0 — the bytes read back from the pipe equal the bytes written
    exit 3 — the round-trip did not match (pipe machinery mis-lowered)

False-pass-proof design:
  * The kernel self-test knows the exact constant the tool sends ("PIPE_OK")
    and asserts the file the tool wrote contains exactly that.  Because the
    write end and read end are *separate* fds connected only by the kernel pipe
    buffer, the bytes can only come back correct if `SYS_PIPE_CREATE` really
    wired a kernel pipe and `write`/`read` really moved data through it — there
    is no userspace echo path a stub could fake.
  * The message is NUL-free ASCII, so it round-trips cleanly through fastpy's
    NUL-terminated `str` value ABI (a NUL-safe bytes overload can come later).

Pure-mode notes (verified bridge-free in the emitted IR — only the
`fpy_cpython_import_native` sentinels from `import os` / `import sys`):
  * `os.pipe()` lowers to a native `fastpy_os_pipe` call returning a 2-element
    list, unpacked by `r, w = os.pipe()`,
  * `os.write`/`os.read`/`os.close` lower to native raw-fd I/O,
  * the string compare is fastpy native string equality.

The kernel embeds the ELF via `include_bytes!` in `kernel/src/proc/spawn.rs`
and runs it as a ring-3 self-test (`self_test_fastpy_slateos_pipe`).

Run with fastpy on PYTHONPATH so `compiler` is importable, from the root of
the worktree you are actually working in. There are four checkouts of this
repo, and naming one of them in a command is how a lane ends up building
another lane's artifact -- see `scripts/lib/worktree.sh`:

    PYTHONPATH="D:/visual studio projects/fastpy" \
        python services/fastpy-pipe/build.py

The posix sysroot (`libc.a`) must already be built; see
`toolchain/build-sysroot.ps1`.
"""

import ast
from pathlib import Path

from compiler.codegen import CodeGen
from compiler import toolchain

# pipe round-trip probe: create a kernel pipe (os.pipe -> SYS_PIPE_CREATE),
# write a known message to the write end and read it back from the read end
# (os.write/os.read -> SYS_FS_WRITE/READ on the pipe fds), close both ends,
# write the round-tripped message to a file the kernel reads back, and exit 0
# iff the bytes survived the pipe intact.
SRC = (
    "import os\n"
    "import sys\n"
    "r, w = os.pipe()\n"
    "msg = \"PIPE_OK\"\n"
    "n = os.write(w, msg)\n"
    "back = os.read(r, 7)\n"
    "os.close(w)\n"
    "os.close(r)\n"
    "f = open('/tmp/fastpy-pipe.out', 'w')\n"
    "f.write(back)\n"
    "f.close()\n"
    "code = 3\n"
    "if back == msg:\n"
    "    if n == 7:\n"
    "        code = 0\n"
    "sys.exit(code)\n"
)


def main() -> None:
    ir = CodeGen().generate(ast.parse(SRC))
    out = Path(__file__).resolve().parent
    out.mkdir(parents=True, exist_ok=True)
    obj = toolchain.compile_ir_to_obj(ir, out / "prog.o", target=toolchain.SLATEOS_TARGET)
    exe = toolchain.link_executable([obj], out / "fastpy-pipe.elf", target=toolchain.SLATEOS_TARGET)
    print("OBJ:", obj, obj.stat().st_size)
    print("EXE:", exe, exe.stat().st_size)


if __name__ == "__main__":
    main()
