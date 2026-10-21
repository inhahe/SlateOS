## D-LLVM-MACHINELICM-SPECULATES-A-GUARDED-LOAD (lane D, 2026-10-21) — OPEN

**Status:** OPEN -- worked around in `libc.a`; the rest of the tree is
exposed

**In short:** the compiler (LLVM, inside rustc, zig and fastpy's llvmlite)
sometimes moves a memory read out of a loop and ahead of the `if` that
protects it. When the protected case is the one that would have read a bad
address, the program crashes on a read it never asked for. It crashed
`msgrcv` in our C library. `libc.a` is now built with that optimisation off.
Everything else built for SlateOS still has it on.

**The bug:** [LLVM issue 233340](https://github.com/llvm/llvm-project/issues/233340)
("Wrong code at -O1 on x86", confirmed miscompilation), fixed by
[PR 234138](https://github.com/llvm/llvm-project/pull/234138), unmerged on
2026-10-21. MachineLICM (the backend pass that hoists loop-invariant
instructions into a loop's preheader) caches whether a block is guaranteed to
execute, once per block. `HoistOutOfLoop` asks that question for the
outermost loop and then, when the hoist fails, for each inner loop, so the
answer for the inner loop ("yes, this block runs every iteration") can be
read back for the outer loop, where it is false. A load is then hoisted
out of the outer loop above the branch that guarded it. Every LLVM since
18.1 has the inner-loop retry: rustc's nightly (LLVM 22.1.4), stable rustc,
zig 0.16's clang and llvmlite 0.48 (LLVM 22.1.0).

**Seen:** `ctest-sysvipc` killed by a page fault in `msgrcv` at
`mov r14,[rcx+rax*8]`, address `0x8002c6d78` (lane-d boot of 2026-10-21,
`build/serial-failures/20261021T081649Z-5a572fd9d-rc1.txt`). `find_msg`'s
`match mode` became a jump table over the four cases after `Search::Any`;
the loop was unswitched on `mode == 0`; and the table load,
`table[mode - 1]` with the subtraction in 32 bits, was hoisted out of
`receive`'s wait loop above the `mode == 0` test. With `msgtyp == 0` the
index is `0xffffffff`, 32 GiB past the table.

**Worked around:** `"llvm-args": ["-disable-machine-licm"]` in
`posix/x86_64-slateos-libc.json`, the spec `libc.a`, `libstubs.a`, `core` and
`compiler_builtins` are compiled for (`toolchain/build-sysroot.ps1` says why
beside the spec's other settings). That switches off the pre-register-allocation
pass, the only one with the inner-loop retry; the post-allocation MachineLICM
asks about one loop per run and stays on. Checked in the rebuilt
`ctest-sysvipc.elf`: the dispatch is `jmpq *(%r11,%r10,8)` again, after the
`mode == 0` test. The cost is invariant instructions LLVM's IR-level LICM
cannot see (with the large code model, every `movabs` of an address) staying
inside loops: a few cheap instructions per iteration.

**Still exposed** (none of it lane D's to change):

| Built by | Owner | Where the flag would go |
|---|---|---|
| rustc for `x86_64-slateos` (every Rust program on the image) | lane A | `toolchain/x86_64-slateos.json`, `"llvm-args"` |
| rustc for the kernel | lane A | the kernel's target spec |
| zig cc (the C fixtures, the ported C programs) | each recipe | `-mllvm -disable-machine-licm` |
| fastpy's llvmlite (fastpy programs) | fastpy | `llvm.set_option` before code generation |

**Remove the workaround when** the nightly in use has an LLVM with PR 234138
(check `rustc +nightly --version --verbose` against the release the PR lands
in), and do the same for each row above.
