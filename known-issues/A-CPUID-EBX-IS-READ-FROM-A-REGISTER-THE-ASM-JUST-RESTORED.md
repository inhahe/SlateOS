## A-CPUID-EBX-IS-READ-FROM-A-REGISTER-THE-ASM-JUST-RESTORED (lane A)

**Status:** FIXED 2026-08-31 in `22506d697` (boot confirmation pending; the fix
is verified in the rebuilt binary — see "Verification" below). **SMEP and SMAP
were disabled on every boot of this kernel between 2026-08-23 and 2026-08-31** —
not because the CPU lacks them, but because the kernel could not see them.

*An earlier revision of this entry said "since the feature-detection code was
written". That was wrong, and the correction makes the bug worse rather than
better.* The retained boot logs show `SMEP=true SMAP=true UMIP=true` on
2026-08-16 (ten soak iterations), 2026-08-18 (two virgl probes) and 2026-08-23
(`serial-batch39.txt`), and `SMEP=false SMAP=false UMIP=true` on 2026-08-31
(`serial-test.txt`). No one touched the detection code in that window — the
leaf-7 parse and the `asm!` block that feeds it both predate 2026-08-16
unchanged. What changed was the register the allocator handed the block, in
response to edits elsewhere in the tree. So this is not a bug that was always
present and always broken; it is a bug that was always present and *became*
broken, silently, as a side effect of unrelated work, and then stayed broken.
That is the actual hazard of the idiom: it does not fail when you write it.

**In short:** To ask the CPU what it supports you run the `cpuid` instruction,
which answers in four registers: EAX, EBX, ECX, EDX. Rust will not let inline
assembly name `ebx` as an output (the compiler reserves that register), so ten
places in this kernel work around it the same way: save `rbx`, run `cpuid`, copy
`ebx` into "some register the compiler picks", restore `rbx`. The bug is the
phrase in quotes. The compiler is free to pick `rbx` itself — nothing told it
not to — and at five places in the compiled kernel it did. The generated code is
`mov %ebx,%ebx` (a no-op) immediately followed by `pop %rbx`, which throws the
answer away and puts back the value `rbx` held *before* the `cpuid`. Every
feature reported by EBX at those sites is read from garbage.

**Evidence, from the shipped binary rather than from reasoning.** Searching
`target/x86_64-unknown-none/release/kernel` for `0f a2` (cpuid) followed by
`mov %ebx,<r32>` and `5b` (pop rbx) finds 21 sites, of which five are the
destructive `mov %ebx,%ebx` — at file offsets `0x1ada19`, `0x1adc1d`,
`0x5b65ba`, `0x10bcbf7` and one more. (Twenty-one machine-code sites from ten
source sites because the helpers are small and get inlined into several callers
each — so one source helper can be correct at one call site and destructive at
another.) The other sixteen picked `esi`/`edi`/`eax` and happen to be correct.
Which register is chosen is an allocation decision that can change with any edit
to surrounding code, so the working sites work by luck, not by construction.

**The observable consequence, and how it was found.** The boot harness runs QEMU
with `-cpu qemu64,+smep,+smap,+umip`, and QEMU accepts all three without warning
(verified directly: realizing a q35 CPU with that exact string emits no
"unsupported feature" diagnostic). The guest nevertheless reports:

    [cpu]   SMEP=false SMAP=false UMIP=true

UMIP is CPUID leaf 7 **ECX** bit 2 and arrives. SMEP and SMAP are leaf 7 **EBX**
bits 7 and 20 and do not. Same leaf, same instruction, same boot — the only
difference is which register the value comes back in, which is exactly the shape
of this bug. `kernel/src/cpu.rs:1057-1059` reads the right bits from the right
registers; the register it reads EBX *from* is the one the asm just restored.

This surfaced from the other end: `[smep_smap] STAC/CLAC, with_user_access and
the access counter` is on `check-boot-skips.py`'s allowlist justified as "needs
SMAP on the running CPU, QEMU does not expose it". That justification is false,
and the entry was hiding a security feature that is off. See "The class gate is
now built" below — this is precisely the failure mode named there as the one an
allowlist has, arriving within a day of the warning being written.

**Blast radius.** Everything the kernel believes about EBX from CPUID:

| Consumer | Reads | Consequence of a false negative |
|---|---|---|
| `smep_smap::init` | SMEP, SMAP | **CR4.SMEP and CR4.SMAP never set.** The kernel can execute and read user pages unhindered — both mitigations inert. |
| `cpu::detect_features` | AVX2, BMI1, BMI2, AVX512F, SHA, RDSEED | Optimised paths silently never taken; RDSEED absence changes entropy sourcing. |
| `cpu_topology` (3 of its 4 sites) | leaf 4 / 0xb EBX | Cache and topology fields read from a restored `rbx`. "Cache topology: not detected" in every log is consistent with this. |

Two consumers that look like they belong in that table do **not**:
`hypervisor.rs`'s leaf-`0x40000000` read and `bench.rs`'s serializing call used
the `xchg rbx, {tmp}` idiom instead, which is correct for either allocation, so
the hypervisor vendor signature was never misread. `mm/pcid.rs` named `eax` and
`ecx` explicitly and was likewise never exposed. All three are still converted —
see "Proper fix" — but they are not part of the damage.

Note the asymmetry that made this invisible: a false *negative* on a CPU feature
disables an optimisation or a mitigation and produces no error, so nothing ever
printed anything red. A false positive would have crashed on the first `stac`.

**Second defect, at a wider set of sites:** twenty-five of the twenty-eight
CPUID call sites in the tree assert `options(nostack)` while containing `push
rbx`/`pop rbx` — including sites that read no EBX at all and were therefore
returning correct answers. That is a direct lie to the compiler: `nostack`
permits red-zone use below `rsp`, which the push would corrupt. The kernel is
built without a red zone so it has not bitten, but it is UB by the `asm!`
contract and must not survive the fix.

**Proper fix.** Delete the hand-rolled save/restore and call
`core::arch::x86_64::__cpuid` / `__cpuid_count` — they are `core` (no `std`
needed), handle the `rbx` problem correctly by construction (the `xchg` idiom is
right for *either* allocation), and have no output operand to mis-allocate.

A first count said eleven sites; grepping for the `"cpuid"` string rather than
for the `ebx_out` operand found twenty-eight, in seven files: `kernel/src/cpu.rs`
(15), `kernel/src/cpu_topology.rs` (4), `kernel/src/cpufreq.rs` (3),
`kernel/src/thermal.rs` (2), `kernel/src/mm/pcid.rs` (2), `kernel/src/bench.rs`
(1), `kernel/src/hypervisor.rs` (1). All twenty-eight are converted, not just
the ten that read EBX: the thing being removed is the idiom, so that the next
person who needs a CPUID has no broken template in the tree to copy. See
design-decisions.md §652.

The conversion also **removes 28 `unsafe` blocks**: `__cpuid`/`__cpuid_count`
are safe functions on x86_64 (CPUID is baseline, so there is no precondition a
caller can get wrong), which the compiler reported as `unnecessary unsafe block`
at every site. The 28 `// SAFETY:` comments above them were never safety
arguments — "Caller verified max_leaf >= 7" says the *answer* is meaningful, not
that the call is sound — so they are kept as plain comments with the `SAFETY:`
marker dropped.

**Reproduce.**

    python -c "import re; b=open('target/x86_64-unknown-none/release/kernel','rb').read(); print(len(re.findall(rb'\x0f\xa2\x89\xdb\x5b', b)), 'destructive cpuid sites')"
    grep -E "SMEP=|SMAP hw=" build/serial-test.txt

**Verification after the fix:** `[cpu]   SMEP=true SMAP=true UMIP=true` under
the harness's default `-cpu`, `[smep_smap] Enabling SMEP` / `Enabling SMAP` in
early boot, and the `[smep_smap]` SKIP disappearing — at which point its
`ALLOWED` entry in `scripts/check-boot-skips.py` must be deleted, because a
stale allowlist entry is itself a hard failure of that gate.

**Verification actually obtained (2026-08-31, commit `22506d697`).** The
allowlist entry is deleted and the rebuilt kernel was scanned with the same
byte-pattern method that found the bug, which is the point: the fix is
confirmed by the same instrument that produced the diagnosis, not by a
different and more forgiving one.

| Pattern following `0f a2` (`cpuid`) | Before | After |
|---|---|---|
| `mov %ebx,<r32>` (the fragile idiom, any allocation) | 21 | **0** |
| `mov %ebx,%ebx` (the destructive allocation) | 5 | **0** |
| `xchg %rbx,<r64>` (`core`'s idiom, correct for any allocation) | — | 144 |

`cargo build -p kernel --release --target x86_64-unknown-none` is clean with
zero warnings. The 144 is larger than the 28 source sites because the helpers
inline into many callers — which is the same reason the "before" count was 21
rather than 10, and is why the binary is the thing to count.

What remains is the boot log, which confirms the *observable* effect rather
than the fix. Note that the failure was value-only, so a passing boot log is
weaker evidence than the byte scan above: a boot that printed `SMEP=true`
would have done so on 2026-08-23 as well, while the bug was fully present.
