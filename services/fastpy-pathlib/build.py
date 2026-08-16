#!/usr/bin/env python3
"""Reproducible build recipe for the `fastpy-pathlib` SlateOS utility.

This produces `fastpy-pathlib.elf`, a native SlateOS (`x86_64-slateos`) binary
compiled by fastpy (AOT Python -> LLVM IR -> native). It is the on-target proof
for fastpy's **pure-mode `pathlib.Path`** runtime (`runtime/pathlib_pure.c` in
the fastpy repo): the CPython-free implementation of `Path(...)`, `write_text`,
`read_text`, `exists`, `is_file`, `is_dir`, `name`, `suffix`, `stem`, `parent`,
and `joinpath` that lets a pure-mode SlateOS program touch the filesystem
through the high-level Pythonic `Path` API (not just the low-level `os.*` calls
the shell-plumbing utilities already proved).

The program exercises the whole surface against a scratch file under `/tmp`,
counting how many independent checks pass, and exits with that count so the
kernel ring-3 self-test (`self_test_fastpy_slateos_pathlib` in
`kernel/src/proc/spawn.rs`) can assert the exact expected total. It takes no
arguments.

The exit status is a **bitmask**: check N (1-based) sets bit (N-1), i.e. adds
2**(N-1) to `ok`. Bit 17 (131072) is a *completion sentinel* set unconditionally
on the last line, so a full pass is 2**18 - 1 == 262143; any other value's clear
bits name exactly which checks failed (far more diagnostic than a bare count),
and a clear bit 17 means the program died before finishing (an uncaught
exception exits 1, which would otherwise read as "only check 1 passed").

Checks (bit value in parentheses; EXPECTED = 262143 on success):
   1. write_text then read_text round-trips the exact bytes        (1)
   2. exists() is true for the written file                        (2)
   3. is_file() is true for it                                     (4)
   4. is_dir() is false for it                                     (8)
   5. name == 'fpy-pathlib.txt'                                    (16)
   6. suffix == '.txt'                                             (32)
   7. stem == 'fpy-pathlib'                                        (64)
   8. str(parent) == '/tmp'                                        (128)
   9. joinpath builds '/tmp/fpy-pathlib.txt' (matches the original)(256)
  10. is_dir() is true for '/tmp'                                  (512)
  11. read_text round-trips on an object-magic-colliding filename  (1024)
  12. .name is intact on that same filename                        (2048)
  13. the os.mkdir'd scratch directory is_dir()                    (4096)
  14. iterdir() entries keep their Path type (.name works on them) (8192)
  15. iterdir() entries are fully joined (read_text on them works)(16384)
  16. the builtin open() accepts a Path (not just a str)          (32768)
  17. a None-returning __exit__ does NOT swallow the exception    (65536)

Checks 11-12 are the on-target half of the FpyPurePath header work: the path
text is chosen so bytes 32..35 spell "SJBO" == FPY_OBJ_MAGIC, which under the
old bare-`char*` Path representation made the OBJ refcount dispatcher treat the
filename as a live object and decrement bytes of it in place. Checks 13-15
cover `Path.iterdir()`, the only Path method returning a container of Paths.

Check 16 covers the builtin `open()`, which reaches the filesystem through
`fastpy_io_open` rather than the `os.*` table swept when the Path header was
introduced — so it kept passing the header through unconverted and fopen() tried
to open a file literally named "PATH" (the header's magic word).

Check 17 is not about pathlib at all; it rides along because it needs a real
boot. A Python method with no `return <expr>` compiles to a *void* LLVM
function, but the runtime's string method dispatcher called every method through
an `int64_t`-returning pointer type, so the "result" was whatever the callee left
in the return register. For `__exit__` a truthy result means "suppress the
exception", so an exception raised in a `with` body silently vanished whenever
`__exit__` did enough work to dirty that register (touching `self` sufficed).
The dispatchers now honour `FpyMethodDef.returns_value`.

Run with fastpy on PYTHONPATH so `compiler` is importable, from the root of
the worktree you are actually working in. There are four checkouts of this
repo, and naming one of them in a command is how a lane ends up building
another lane's artifact -- see `scripts/lib/worktree.sh`:

    PYTHONPATH="D:/visual studio projects/fastpy" \
        python services/fastpy-pathlib/build.py

The posix sysroot (`libc.a`) and the pure-mode SlateOS runtime objects are
built on demand by the fastpy toolchain.
"""

import ast
from pathlib import Path

from compiler.codegen import CodeGen
from compiler import toolchain

# Each satisfied check adds its unique power-of-two bit to `ok`; the process
# exits with `ok` (a bitmask). Kept as a flat sequence of `if cond: ok = ok + N`
# (no boolean fold) to stay squarely in pure-mode-safe codegen territory — the
# same discipline the shell-plumbing utilities used.
SRC = (
    "import os\n"
    "import sys\n"
    "from pathlib import Path\n"
    "p = Path('/tmp/fpy-pathlib.txt')\n"
    "p.write_text('hello pathlib\\n')\n"
    "ok = 0\n"
    "if p.read_text() == 'hello pathlib\\n':\n"
    "    ok = ok + 1\n"
    "if p.exists():\n"
    "    ok = ok + 2\n"
    "if p.is_file():\n"
    "    ok = ok + 4\n"
    "if not p.is_dir():\n"
    "    ok = ok + 8\n"
    "if p.name == 'fpy-pathlib.txt':\n"
    "    ok = ok + 16\n"
    "if p.suffix == '.txt':\n"
    "    ok = ok + 32\n"
    "if p.stem == 'fpy-pathlib':\n"
    "    ok = ok + 64\n"
    "d = p.parent\n"
    "if str(d) == '/tmp':\n"
    "    ok = ok + 128\n"
    "j = d.joinpath('fpy-pathlib.txt')\n"
    "if str(j) == '/tmp/fpy-pathlib.txt':\n"
    "    ok = ok + 256\n"
    "if d.is_dir():\n"
    "    ok = ok + 512\n"
    # Checks 11-12: bytes 32..35 of this path spell "SJBO" == FPY_OBJ_MAGIC.
    # "/tmp/" (5) + 27 filler puts the magic exactly at the offset the OBJ
    # refcount dispatcher probes.
    "c = Path('/tmp/aaaaaaaaaaaaaaaaaaaaaaaaaaaSJBO-collide.txt')\n"
    "c.write_text('collide\\n')\n"
    "if c.read_text() == 'collide\\n':\n"
    "    ok = ok + 1024\n"
    "if c.name == 'aaaaaaaaaaaaaaaaaaaaaaaaaaaSJBO-collide.txt':\n"
    "    ok = ok + 2048\n"
    # Checks 13-15: iterdir(). Start from a known-empty directory so the entry
    # count is exact. os.remove/os.rmdir return -1 instead of raising when the
    # target is absent, so no guards are needed on a first run.
    #
    # The program also prints DIAG lines to stdout (which the harness captures
    # on the serial log). A bitmask alone cannot distinguish "os.mkdir failed"
    # from "opendir failed" from "entries came back unjoined", and each on-target
    # iteration costs a full boot — so the setup steps announce themselves.
    "os.remove('/tmp/fpy-iterdir/one.txt')\n"
    "os.remove('/tmp/fpy-iterdir/two-with-a-long-entry-name.txt')\n"
    "os.rmdir('/tmp/fpy-iterdir')\n"
    "os.mkdir('/tmp/fpy-iterdir')\n"
    "e = Path('/tmp/fpy-iterdir')\n"
    # Check 13 is now "the scratch dir exists", so a mkdir failure is reported
    # as itself instead of masquerading as an iterdir failure.
    "if e.is_dir():\n"
    "    ok = ok + 4096\n"
    "    print('DIAG mkdir-ok')\n"
    "if not e.is_dir():\n"
    "    print('DIAG mkdir-FAILED')\n"
    "e.joinpath('one.txt').write_text('1\\n')\n"
    "e.joinpath('two-with-a-long-entry-name.txt').write_text('2\\n')\n"
    "n = 0\n"
    "named = 0\n"
    "body = ''\n"
    "for f in e.iterdir():\n"
    "    n = n + 1\n"
    "    print(str(f))\n"
    "    if f.name == 'one.txt':\n"
    "        named = named + 1\n"
    "    if f.name == 'two-with-a-long-entry-name.txt':\n"
    "        named = named + 1\n"
    "    body = body + f.read_text()\n"
    "print('DIAG iterdir-count')\n"
    "print(str(n))\n"
    "if named == 2:\n"
    "    ok = ok + 8192\n"
    "if body == '1\\n2\\n' or body == '2\\n1\\n':\n"
    "    ok = ok + 16384\n"
    # Check 16: the builtin open() given a Path rather than a str. This is a
    # different code path from os.open() — it goes to fastpy_io_open — and it was
    # missed when the FpyPurePath header landed, so fopen() received the header
    # and reported "No such file or directory: 'PATH'".
    "fh = open(p, 'r')\n"
    "if fh.read() == 'hello pathlib\\n':\n"
    "    ok = ok + 32768\n"
    "fh.close()\n"
    # Check 17: a `with` block whose __exit__ returns None must NOT suppress an
    # exception raised in the body. __exit__ touches `self` deliberately — that
    # is what used to leave a non-zero value in the return register, which the
    # `with` lowering then read as a truthy "suppress it" answer.
    "class Guard:\n"
    "    def __init__(self, tag):\n"
    "        self.tag = tag\n"
    "    def __enter__(self):\n"
    "        return self\n"
    "    def __exit__(self, a, b, c):\n"
    "        print('DIAG guard-exit')\n"
    "        print(self.tag)\n"
    "caught = 0\n"
    "try:\n"
    "    with Guard('g1'):\n"
    "        raise ValueError('propagate me')\n"
    "except ValueError:\n"
    "    caught = 1\n"
    "if caught == 1:\n"
    "    ok = ok + 65536\n"
    "if caught == 0:\n"
    "    print('DIAG with-exception-SWALLOWED')\n"
    # Completion sentinel (bit 17). Set unconditionally on the last line before
    # exiting, so its ABSENCE means "the program never reached the end" — an
    # uncaught exception, which exits 1, would otherwise be indistinguishable
    # from the legitimate bitmask "only check 1 passed".
    "ok = ok + 131072\n"
    "sys.exit(ok)\n"
)


def main() -> None:
    ir = CodeGen().generate(ast.parse(SRC))
    out = Path(__file__).resolve().parent
    out.mkdir(parents=True, exist_ok=True)
    obj = toolchain.compile_ir_to_obj(ir, out / "prog.o", target=toolchain.SLATEOS_TARGET)
    exe = toolchain.link_executable([obj], out / "fastpy-pathlib.elf", target=toolchain.SLATEOS_TARGET)
    print("OBJ:", obj, obj.stat().st_size)
    print("EXE:", exe, exe.stat().st_size)


if __name__ == "__main__":
    main()
