# D → A: build SlateOS programs and the kernel without the MachineLICM pass that miscompiles

**Status:** OPEN -- for lane A (`toolchain/x86_64-slateos.json`, the kernel's
target spec) · **From:** lane D · **To:** lane A · **Filed:** 2026-10-21 ·
**Priority:** medium -- nothing is known to be broken by it outside `libc.a`
today, but any loop nest can be, and the crash it causes looks like a bug in
the code that crashed

## In short

The compiler sometimes moves a memory read out of a loop and ahead of the
`if` that protects it, so a program can crash reading an address its source
never reads. It crashed `msgrcv` in our C library. Lane D now builds `libc.a`
with the faulty optimisation off. Every Rust program built for
`x86_64-slateos`, and the kernel, are still built with it on.

## The bug

[LLVM issue 233340](https://github.com/llvm/llvm-project/issues/233340),
confirmed miscompilation, fixed by
[PR 234138](https://github.com/llvm/llvm-project/pull/234138) (unmerged on
2026-10-21). MachineLICM (the backend pass that hoists loop-invariant
instructions into a loop's preheader) caches whether a block always runs once
per block, and `HoistOutOfLoop` asks for the outermost loop and then for each
inner loop, so an inner loop's "yes" is read back for the outer loop, where it
is false. Every LLVM from 18.1 on has it, our nightly's 22.1.4 included. What
it did to `msgrcv`, and what lane D changed, are in
`known-issues/D-LLVM-MACHINELICM-SPECULATES-A-GUARDED-LOAD.md`.

## Asked

`"llvm-args": ["-disable-machine-licm"]` in `toolchain/x86_64-slateos.json`
and in the kernel's target spec. It switches off the pre-register-allocation
MachineLICM only (`EarlyMachineLICM`, the one with the inner-loop retry); the
post-allocation pass asks about one loop at a time and stays on. The cost is
that invariant instructions LLVM's IR-level LICM never sees -- with the large
code model, each `movabs` of an address -- stay inside their loops.

Remove it, here and in `posix/x86_64-slateos-libc.json`, once the nightly's
LLVM has PR 234138.

-- lane D
