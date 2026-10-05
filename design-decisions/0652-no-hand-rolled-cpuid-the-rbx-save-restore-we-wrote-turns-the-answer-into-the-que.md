## 652. No hand-rolled CPUID: the `rbx` save/restore we wrote turns the answer into the question, and the kernel believed it had no SMEP

**Date:** 2026-08-31
**Lane:** A
**Decided by:** Claude (autonomous), lane A

**In short:** `CPUID` is the x86 instruction that asks the processor what
features it has — SMEP and SMAP among them, two hardware defences that stop
kernel code being tricked into running or reading user memory. CPUID answers in
four registers, one of which is `EBX`, and Rust will not let you name `EBX` as
an output because the compiler reserves it for its own use. So every CPUID call
in this kernel worked around that by hand. The workaround was wrong: at five
places in the compiled kernel the compiler happened to hand us `EBX` itself as
the "somewhere else" to copy the answer into, which made the copy a no-op and
then overwrote the answer with the saved value. The kernel therefore read its
own saved register and concluded that the CPU had no SMEP and no SMAP — on a
QEMU CPU that was explicitly configured to have both. Every CPUID in the tree
now goes through `core`'s own `__cpuid` / `__cpuid_count`, which cannot fail
this way.

### The bug

Ten of the twenty-eight CPUID call sites needed `EBX` and asked for it like
this:

```rust
core::arch::asm!(
    "push rbx",
    "mov eax, 7", "xor ecx, ecx",
    "cpuid",
    "mov {ebx_out:e}, ebx",   // copy the answer out of ebx...
    "pop rbx",                // ...then restore rbx
    ebx_out = out(reg) ebx,
    // ...
    options(nomem, nostack),
);
```

`out(reg)` means "any general-purpose register the allocator likes". `rbx` is
in that class. When the allocator picks it, the two instructions become

```
mov %ebx,%ebx     # no-op
pop %rbx          # answer destroyed, replaced by the pre-CPUID value
```

and the function returns whatever `rbx` held on entry. Nothing warns. The code
reads correctly, the register names are right, and the failure is invisible
except in the *value* — which is exactly the kind of thing a feature-detection
routine has no way to sanity-check, because "the CPU does not have this
feature" is a completely ordinary answer.

**How it was found, which matters more than the bug.** Not by reading the
assembly for fun. §651's gate carries an allowlist, and each entry must state
the observable condition under which its skip would stop firing. The entry for
`[smep_smap] STAC/CLAC, with_user_access and the access counter` said "QEMU's
default TCG CPU model does not expose SMAP". Checking that claim rather than
trusting it found that `scripts/boot-test.sh` has passed `-cpu
qemu64,+smep,+smap,+umip` since lane B's `B-QEMU-DEFAULT-CPU-HAS-NO-SMEP-SMAP-UMIP`
fix, and that QEMU accepts all three without a warning. The boot log then read
`SMEP=false SMAP=false UMIP=true` — and UMIP is CPUID leaf 7 **ECX** bit 2
while SMEP and SMAP are leaf 7 **EBX** bits 7 and 20. One register arriving and
its neighbour not is not a CPU-model fact; it is a register-handling fact. That
asymmetry is the entire diagnosis, and it came out of an allowlist entry being
made to justify itself.

**Confirmed in the shipped binary, not by reasoning.** Those ten source sites
are `#[inline]`-eligible helpers called from several places each, so they expand
to more than ten machine-code sites. Searching
`target/x86_64-unknown-none/release/kernel` for `0f a2` (`cpuid`) followed by a
`mov %ebx,<r32>` and a `5b` (`pop %rbx`) finds twenty-one, of which **five** are
the destructive `mov %ebx,%ebx`. The other sixteen drew `esi`/`edi`/`eax` and
are correct **by luck**: which register the allocator picks is a decision that
can change with any edit to surrounding code, so a site that works today can
start returning garbage because someone added a local variable two functions
away. The same source helper can even be right at one call site and wrong at
another, which is why the count in the binary is the count that matters.

**"Correct by luck" is not a theoretical worry here — the luck ran out, and the
boot logs date it.** Every retained serial log that reports the line is
unanimous until the last one:

| Log | Date | `[cpu]   SMEP=… SMAP=… UMIP=…` |
|---|---|---|
| `hang-catches/soak-20260816-143012-iter01..10` | 2026-08-16 | `SMEP=true SMAP=true UMIP=true` |
| `virgl-probe-gl.txt`, `virgl-probe-plain.txt` | 2026-08-18 | `SMEP=true SMAP=true UMIP=true` |
| `serial-batch39.txt` | 2026-08-23 | `SMEP=true SMAP=true UMIP=true` |
| `serial-test.txt` | 2026-08-31 | **`SMEP=false SMAP=false UMIP=true`** |

Nobody edited the feature-detection code in between. `cpu.rs`'s leaf-7 parse —
`f.smep = ebx7 & (1 << 7) != 0` and the SMAP bit beside it — is untouched since
long before 2026-08-16, and so is the `asm!` block that feeds it. What changed
was the register the allocator handed that block, in response to edits
elsewhere. So the kernel's two anti-privilege-escalation defences switched
themselves off somewhere between 2026-08-23 and 2026-08-31, silently, as a
side effect of unrelated work, and stayed off through every boot since.

That is the strongest argument in this entry, and it is an observation rather
than an argument: an idiom whose correctness is decided by register allocation
does not fail when you write it, it fails later, for reasons that have nothing
to do with it, and it takes a security feature with it. Ten sites were one
unlucky allocation away from the same thing at all times.

### The decision

Every CPUID in the kernel calls `core::arch::x86_64::__cpuid` or
`__cpuid_count`. Twenty-eight sites across seven files: `cpu.rs` (15),
`cpu_topology.rs` (4), `cpufreq.rs` (3), `thermal.rs` (2), `mm/pcid.rs` (2),
`bench.rs` (1), `hypervisor.rs` (1). Ten of those needed `EBX` and were exposed
to the bug; the other eighteen are converted because the idiom is what is being
removed, not the eighteen individual bugs it did not happen to cause.

`core`'s implementation uses the `xchg` idiom rather than push/pop:

```
mov  {tmp:r}, rbx
cpuid
xchg {tmp:r}, rbx
```

which is correct for *either* allocation — if `tmp` is `rbx` both instructions
are no-ops and `tmp` ends up holding CPUID's `EBX`; if it is anything else the
`xchg` swaps the answer out and the saved value back. There is no allocation
that loses the result. This is a property of the instruction sequence, not of
the allocator's mood, which is the whole reason to prefer it.

**An unbudgeted dividend: 28 `unsafe` blocks disappear.** `__cpuid` and
`__cpuid_count` are *safe* functions on `x86_64` — CPUID is baseline on the
architecture, so there is no target-feature precondition for a caller to get
wrong, and the compiler said so by emitting `unnecessary unsafe block` at all 28
sites on the first build. Every one of the 28 `// SAFETY:` comments that used to
sit above them was, on inspection, not a safety argument at all: "Caller
verified max_leaf >= 7" is a statement about whether the *answer is meaningful*,
not about whether the call is sound. They are retained as plain comments,
because the precondition is still worth stating, and the `SAFETY:` marker is
removed because a marker on a claim that justifies nothing is how a reader
learns to skim them. This is a real reduction in the kernel's unsafe surface as
a side effect of fixing a correctness bug, and it is the strongest argument
against ever hand-rolling this again: the hand-rolled form *required* `unsafe`
and then used it to be wrong.

*Alternative rejected: fix the five broken sites.* It restores SMEP and SMAP
today and leaves sixteen sites that are correct for a reason nobody can state
and nothing enforces. The bug is not "five sites are wrong"; it is "the idiom
is wrong and twenty-one sites use it".

*Alternative rejected: keep `asm!` but write the `xchg` form ourselves.* Two of
the twenty-eight sites already did (`hypervisor.rs`, `bench.rs`) and were
correct. Both are converted anyway, because leaving any hand-rolled CPUID in
the tree leaves a template for the next person who needs one — and the evidence
of this entry is that the template that got copied twenty-five times was the
broken one.

### A second defect the same edit removes

Twenty-five of the twenty-eight sites asserted `options(nostack)` while
executing `push rbx`.
`nostack` is a promise to the compiler that the block does not touch the stack,
which licenses it to keep live data in the 128-byte red zone below `rsp`. A
`push` writes exactly there. This is undefined behaviour by the `asm!`
contract, independent of the `rbx` bug and present even at the sites that read
`EBX` correctly, including the ones that read no `EBX` at all. It is not known
to have caused a miscompilation here — the kernel is built without red-zone use
in most configurations — but "we have not been bitten yet" is not a property
that survives a compiler upgrade. `__cpuid` executes no `push`, so its
`nostack` is true.

### Two discarded results that must not be discarded

`cpu::serialize()` and `bench::serialize()` call CPUID purely for its
serializing side effect and throw the registers away. That is safe only because
`__cpuid`'s inner `asm!` does not carry `options(pure)`, so LLVM must assume
unknown effects and cannot elide the call. Both sites say so in a comment,
because the failure mode if that ever changed is silent in the worst way:
`cpu::serialize` would stop flushing the prefetch queue after
`alternatives::apply` patches `.text`, leaving the CPU free to execute the
*stale* bytes, and `bench::serialize` would leave every microbenchmark's
`rdtsc` unserialized and its numbers quietly wrong.

### What changes observably

`[cpu]   SMEP=true SMAP=true UMIP=true` under the harness's default `-cpu`,
`[smep_smap] Enabling SMEP` / `Enabling SMAP` in early boot, and the
`[smep_smap] STAC/CLAC, with_user_access and the access counter` skip stops
firing — so its `ALLOWED` entry in `scripts/check-boot-skips.py` is deleted in
the same commit. §651's gate treats a stale allowlist entry as a hard failure
precisely so that a fix cannot leave a false statement behind it. The two
security features were configured, believed to be present by the operator, and
not actually on; they are now on.

The `[pcid]` allowlist entry stays. Its two CPUID reads named `eax` and `ecx`
explicitly, so the allocator had no freedom and the answers were always
correct: PCID really is absent from `qemu64`, for the reason the entry gives.

### How to reverse it

There is no reason to. If `core::arch::x86_64::__cpuid` ever became unavailable
in a `no_std` kernel build, the replacement is the `xchg` sequence above written
out by hand in **one** helper that everything else calls — not twenty-eight
copies, and never the `push`/`mov`/`pop` form, which is the form this entry
exists to bury.
