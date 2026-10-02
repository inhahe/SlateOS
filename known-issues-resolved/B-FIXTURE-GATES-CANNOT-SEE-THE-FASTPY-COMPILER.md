## B-FIXTURE-GATES-CANNOT-SEE-THE-FASTPY-COMPILER (lane B) — fixed 2026-08-22

**In short:** 61 of the 70 ring-3 test binaries are Python compiled to native
code by fastpy. Two separate checks are supposed to notice when one of those
binaries is out of date and needs rebuilding. Neither of them looked at *the
compiler* — so when fastpy fixed a bug, all 61 binaries silently kept the bug
and both checks kept saying they were fine. One of them was a self-test that
had been failing for a day (`BUG-FASTPY-MINISHELL-EXITS-0-WITHOUT-FORKING`).

**Where it lived.** Two gates, one defect:

| Gate | What it listed as an input |
|---|---|
| `scripts/ctest-fixtures.py::_inputs` | `build.py`, `main.c`, `*.h`, `toolchain/sysroot/lib/libc.a` |
| `scripts/create-ext4-rootfs.sh`, the completeness/staleness loop | the same four |

For a **C** fixture that list is complete: `main.c` is the source and `zig cc`
is a fixed toolchain. For a **fastpy** fixture it is missing the largest input
there is. The fixture's whole source is a string literal inside its `build.py`,
so the source genuinely never moves; what moves is `compiler.codegen`, which
turns that unchanged source into a different binary. An input that is not
listed is an input that cannot make anything stale.

**How it was found.** By asking why a fixture whose bug was fixed upstream at
18:03 was still built at 16:05 and still called current. The answer was not
that the gate was wrong about the files it checked — it was right about all
four — but that the set was short.

**The fix.** Both gates now include the newest `.py` under fastpy's `compiler/`
package as an input to every `fastpy-*` fixture, using the same mtime ordering
as every other input, and reporting it by *file name* so the diagnosis says
which file moved rather than "fastpy changed". The compiler checkout is located
exactly as the builder locates it (`$FASTPY_DIR`, then `$PYTHONPATH`, then a
sibling of the repo root named exactly `fastpy`), because a gate that resolves
its inputs differently from the builder is a gate that judges a different
artifact than the one that gets built. Not finding a checkout omits the input
rather than failing closed: the shell gate runs under WSL, which cannot rebuild
a fastpy fixture in any case, and a machine with no fastpy at all should not be
blocked from packing an image.

**The measurement that shows the size of the hole:** adding the input reported
**61 of 70 fixtures stale** on the first run — i.e. every fastpy fixture in the
tree, and none of the nine C ones. All 61 were rebuilt.

**Related, and the same lesson twice.** §355 removed these ELFs from git and
made them build on demand, on the argument that *a gate cannot tell which side
moved, but a build step never has to ask*. This is the other half of the same
observation: a build step can only rebuild what a staleness answer names, so
the answer has to be complete. The rootfs script's own comments already warn
against "a per-family gate in a per-family loop" producing a per-family blind
spot; this was a per-*input-kind* blind spot, produced the same way — the input
list was written for the C family and reused unchanged for the fastpy one.
