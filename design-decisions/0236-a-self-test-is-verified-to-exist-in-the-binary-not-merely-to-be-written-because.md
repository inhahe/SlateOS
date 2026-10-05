## §236 — A self-test is verified to *exist in the binary*, not merely to be written, because the optimiser is entitled to answer the question for you

**Date:** 2026-08-19
**Decided by:** Claude (autonomous)
**Lane:** A

**In short:** The kernel has a check that runs at boot and confirms the layout
padding landed where the linker script says it should. On 2026-08-19 that check
failed on every padded build — printing two addresses that were *identical* and
declaring them different. The compiler had decided the answer at build time
(wrongly) and deleted the half of the check that says "OK", so the check could
only ever fail. We fixed the check, and then added a rule: from now on the
build-time test suite confirms that each branch of that check is still *present
in the compiled kernel*, not just present in the source.

### The problem

LLVM guarantees that two distinct global objects have distinct addresses, and
uses that to constant-fold address comparisons. `layout_pad.rs` declared
`__text_start` and `__layout_pad_start` as two `extern static u8`s that
`linker.ld` deliberately makes *the same address*. So `pad_start != text_start`
folded to `true`, the success branch became unreachable, and LLVM removed it.
`main.rs` halts on failure by design, so every optimised nonzero-pad kernel
wedged at boot. Full write-up in `known-issues.md`.

The check was correct as written, correct in intent, and had a correct ELF
underneath it. It still could not pass. That is the part worth turning into a
rule: **source-level correctness of a self-test says nothing about whether the
self-test survived compilation.**

### Decision 1 — launder linker-symbol addresses through an `asm!` barrier, not `black_box`

`opaque_addr()` passes the address through an empty `asm!` with the value as an
`inout(reg)` operand. The compiler must assume the block wrote an arbitrary
integer, so everything it knew about the symbol — including that it *was* a
symbol — is gone, and the comparison has to happen at run time.

*Alternative considered: `core::hint::black_box`.* Shorter, no `unsafe`, and it
would almost certainly work on today's rustc, which lowers it to the same
barrier. Rejected because its documentation states plainly that it guarantees
nothing and may be a no-op. The cost of that being wrong is not a slow kernel;
it is this exact bug returning silently on a toolchain upgrade, as an
unbootable kernel whose source looks fine. An `unsafe` block with a SAFETY
comment is a real price, but it buys a guarantee rather than a likelihood, and
the failure it prevents is one we have already paid for once.

*Alternative considered: make it one symbol.* `linker.ld` could define only
`__text_start` and have the pad module use it for both roles. That removes the
aliasing — and with it the entire check, since "is the pad at the start of
.text" becomes a tautology. The check exists precisely because `linker.ld` is
editable and a future toolchain could reorder sections. Deleting the question
to avoid a wrong answer is the worst available option.

### Decision 2 — verify branch survival by string presence, and extract the strings

`--self-test` now requires every message `self_test_pad_is_first_in_text()` can
print to appear in each padded release image. A branch that has been folded
away takes its message with it, because nothing else references the string.

*Why this proxy rather than something stronger.* The rigorous version is to
disassemble the function and confirm a `cmp` of two run-time values survives.
That is a real check on the real artifact, and it is also a fragile one: it
depends on instruction selection, on which registers the barrier chose, and on
the function still being a discrete function rather than inlined into
`kernel_main`. It would need maintenance every toolchain bump, and a check that
cries wolf gets suppressed. String presence has none of those failure modes: it
is one `in` against the image bytes, and it is *exactly* correlated with the
thing we care about, because a message string is referenced by precisely one
branch and nothing else.

*Why the strings are extracted from the Rust source rather than written down in
the Python.* Same reasoning as `extract_dependency_probe()` (see §235's
neighbours): a second copy of a fact drifts, and a drifted copy checks
something that is no longer true. Extraction makes rewording a message a
one-edit change, and makes *deleting* a branch — the thing being guarded
against — show up as a count mismatch that the extractor refuses to paper over.

### The general rule this establishes

Where a self-test's outcome could be decided at compile time, the test suite
must confirm the test still exists in the artifact. Concretely, in this repo
that means: any check comparing two linker symbols, any check whose operands
are all compile-time constants, and any check inside `#[cfg]`-gated code that
could be gated out. The existing project rule is "a check that cannot fire
presents as a check that found nothing"; this is its twin — *a check the
compiler has already answered presents as a check that ran.*

### Consequence

An audit of every other linker-symbol use in the kernel (`alternatives.rs`,
`idt.rs`, `mm/protect.rs`) found no other site with this exposure: every one of
them compares a symbol against a **run-time** value (a page-table walk's
address, a stored callback pointer), which LLVM cannot fold, or subtracts the
bounds of a section that is never empty. `layout_pad.rs` was the only place
where both sides of a comparison were linker symbols, and it is also the only
place where the linker script *intends* two symbols to coincide — which is what
made it the one that broke. Those two facts travel together, and the second is
the cheap thing to grep for next time.
