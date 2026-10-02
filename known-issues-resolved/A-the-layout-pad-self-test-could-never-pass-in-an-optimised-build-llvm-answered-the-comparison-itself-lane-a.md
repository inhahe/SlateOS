## The layout-pad self-test could never pass in an optimised build: LLVM answered the comparison itself (lane A)

**Status:** FIXED 2026-08-19
**Where:** `kernel/src/layout_pad.rs` (`self_test_pad_is_first_in_text`,
`pad_bytes`), `kernel/linker.ld`, `scripts/layout-sweep.py` (`--self-test`)
**Found by:** the six-arm release layout sweep, which died on its second arm
after a 43-minute build.

### Symptom

```
[boot] build profile: sanitizer=none textpad=1024
[layout_pad] FAIL: pad at 0xffffffff80001000 but .text starts at 0xffffffff80001000
             — linker.ld is not placing .text.slateos_layout_pad first, ...
BENCH_OK not found within 1200s.
```

The two addresses in that message are *the same address*. They are identical
byte-for-byte under `cat -A`. The check compared them, took the not-equal
branch, and printed both — with the same value in each.

`main.rs` halts on that failure, deliberately (a misplaced pad makes every
sweep sample a subset of every other one, and numbers that under-report layout
sensitivity are worse than no numbers because they are used to dismiss real
regressions). So the kernel wedged, the boot test timed out at 1200 s, and the
sweep stopped.

### Root cause

LLVM's object model guarantees that two **distinct global objects** occupy
**distinct addresses**. That is a licence to constant-fold `&A == &B` to
`false` without consulting the linker.

`__text_start` and `__layout_pad_start` are two distinct `extern static u8`
declarations. They are also, by construction, *the same address* —
`linker.ld` emits them back to back with nothing in between:

```
.text : ALIGN(4K) {
    __text_start = .;
    __layout_pad_start = .;
    KEEP(*(.text.slateos_layout_pad))
```

So the source asked "are these two addresses equal?", the optimiser read it as
"are these two distinct globals the same object?", answered *no* at compile
time, and deleted the equal-branch as dead code. The check had become a
constant `FAIL`.

The decisive evidence, which is worth repeating because address-printing alone
looks like a linker bug and sent the first hour in the wrong direction:

| grep of the padded release binary | count |
|---|---|
| `linker.ld is not placing` (the FAIL string) | 1 |
| `pad byte(s) at the head of .text` (the OK string) | **0** |
| `no padding in this build` | 1 |

The success message is not in the image. Nothing references it, because the
branch that printed it does not exist. The ELF itself was entirely correct
throughout: `__text_start` = `__layout_pad_start` = `ffffffff80001000`,
`__layout_pad_end` = `ffffffff80001400`, i.e. exactly the 1024 bytes requested,
placed first.

### Why it survived until the second arm of a real sweep

Three filters, each of which independently hides it:

- **`layout-sweep.py --self-test` checks the ELF, not the kernel.** It verifies
  the pad symbol sits exactly at `__text_start` — reading the symbol table on
  the host. That claim was true. It said nothing about whether the code that
  makes the same claim *on the target* still existed. A 34-minute self-test
  passed on a kernel that could not boot.
- **Arm 1 uses `pad=0`,** which takes the early `if pad == 0` return before
  reaching the comparison. The sweep's first arm therefore always passes.
- **A debug build does not fold it.** Every interactive test of this module had
  been a debug build or an unpadded one.

So the first optimised, nonzero-pad *boot* is the first moment the bug can
appear, and that is exactly where it appeared.

### This is the project's signature defect, inverted

The recurring failure class here is *a check that cannot fire, presenting as a
check that found nothing*. This is the mirror image: **a check that always
fires, regardless of the evidence.** It is the rarer and, in one respect, the
kinder form — it is loud. But it is worse in another: because the failure is
fatal by design, an always-firing check does not merely mislead, it bricks
every build it is compiled into.

Both forms have the same underlying shape — *the answer was decided somewhere
other than where the question was asked.* In the classic form the decision is
made by a guard that never runs; here it is made by the optimiser.

### The fix

`opaque_addr()` in `layout_pad.rs`: read each linker symbol's address through
an empty `asm!` block with the value as an `inout(reg)` operand.

```rust
fn opaque_addr(symbol: *const u8) -> usize {
    let mut addr = symbol as usize;
    // SAFETY: the template assembles to nothing ...
    unsafe {
        core::arch::asm!("/* {0} */", inout(reg) addr,
                         options(nomem, nostack, preserves_flags));
    }
    addr
}
```

The compiler must assume the block wrote an arbitrary integer into that
register, so nothing it knew about the symbol survives — including that it was
a symbol at all. The comparison then happens at run time against what the
linker actually emitted, which is the only place the question has an answer.
Applied to both `self_test_pad_is_first_in_text` and `pad_bytes` (the latter
has not been observed folding, but rests on the identical assumption and would
fail *silently*, with a wrong length, rather than loudly).

Notes on the shape of it:

- The template is `"/* {0} */"`, an assembler comment, not `""`. rustc rejects
  an operand the template never names; the comment assembles to nothing.
- Deliberately **not** `options(pure)`. Purity would let the optimiser reason
  about the result again, which is the entire thing being prevented.
- **Not `core::hint::black_box`.** It would very likely work today, but its
  documentation is explicit that it guarantees nothing and may become a no-op.
  A check that silently reverts to always-failing on a compiler upgrade is not
  a check — and reverting is precisely what it would do here.

### The regression test, and why it is a string search

`--self-test` gained a fourth claim: after each padded build, **every branch of
`self_test_pad_is_first_in_text` must still be present in the optimised
image**. A folded-away branch takes its message with it, since nothing else
references the string — so string-presence is a direct, cheap proxy for "the
comparison is still being made at run time". This is the only claim in the
self-test that is about the *kernel* rather than about the ELF, which is
exactly the gap that let the bug through.

The strings are **extracted from `layout_pad.rs`, not copied** into the Python
(`on_target_messages()`), for the same reason `extract_dependency_probe()`
extracts the QEMU search rather than restating it: a copy drifts, and a drifted
copy checks a message the kernel no longer prints. Rewording a message updates
the check in the same edit; *deleting* a branch makes the count drop and the
extractor refuse. It takes the longest literal fragment between `{}`
placeholders, because `core::fmt` stores a format string as separate pieces —
a whole formatted message is never contiguous in the binary and searching for
one would never match.

Verified in both directions: the fixed pad=1024 release binary contains all
three messages; a deliberately reverted build (raw `addr_of!`, no barrier) is
missing the success message and the new claim fails.

### If you are comparing two linker symbols

Assume the optimiser will answer for you. Two `extern static` declarations are
two distinct globals as far as LLVM is concerned, whatever the linker script
does with them — so any comparison, subtraction or aliasing question about them
must be laundered through an optimisation barrier first. Symbols that a linker
script *intends* to alias are the dangerous case, because the code reads
correctly and the ELF is correct; only the compiled kernel is wrong.
