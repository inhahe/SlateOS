## §237 — The benchmark record stores which emulator it ran on, taken from the guest's own boot banner, and that value is part of the key that groups records for comparison

**Date:** 2026-08-19
**Decided by:** Claude (autonomous)
**Lane:** A

**In short:** Our benchmarks run inside an emulated PC, and there are two very
different ways to emulate one: QEMU can interpret every instruction in software
(TCG), or it can hand the instructions to the CPU's own virtualization hardware
(WHPX). We measured the *same kernel binary* both ways: the typical benchmark
runs 3.5× faster under the hardware one, the best 10×. Until now the saved
result did not say which had been used, so a comparison could unknowingly put a
software-emulated run next to a hardware-accelerated one and report a tenfold
speed difference as though it were a change in our code. Two decisions: record
which emulator was used, reading it from what the kernel itself printed at boot
rather than from the flag we passed; and make that value part of the identity of
a comparison group, so a mixed comparison cannot be constructed at all.

### The measurement that forced it

Run 2026-08-19, one byte-identical kernel (`kernel_sha 7a17cf6be2a10a26` —
literally the same bytes on disk, so no code difference and no code *placement*
difference), release profile, 86 benchmarks, on this host:

| | |
|---|---|
| Median | **×3.53** faster under WHPX |
| Faster under WHPX | 82 of 86 |
| Slower under WHPX | 4 of 86 |
| Best | `ipc_channel_roundtrip_64k` ×10.36 (23352 ns → 2253 ns) |
| Worst | `hpet_read` ×0.033 (453 ns → 13534 ns) |

The four that get *slower* are not noise and they are not a puzzle — they are a
signature. `hpet_read`, `net_arp_lookup` and `net_ns_arp_lookup` all collapse to
almost exactly the same number, ~13.5 µs, regardless of what they were before.
That is the cost of one VM exit: reading an emulated timer device under WHPX
traps out to the hypervisor, where TCG, already in software, simply computes the
answer inline (~450 ns). The two ARP benchmarks are slow for the same reason —
they read the clock. So the accelerator does not scale performance, it *reshapes*
it: everything CPU-bound gets much faster, everything device-bound gets ~30×
slower.

`isr_latency` at ×0.862 is the control that proves the rest are real. If WHPX
had merely rescaled our timing calibration, every benchmark would have moved by
one common factor. Instead one barely moves while another drops 87%. The
differences are differences in what the machine actually does.

### Correction: the accelerator was not the *only* difference

Noticed while writing the follow-up question, and recorded here rather than
quietly fixed, because the original phrasing of this section ("the same bytes, so
there is no code difference") was an overclaim about what *executed*.

The two boots differ in three CPU features as well as the accelerator:

| | TCG | WHPX |
|---|---|---|
| SMEP / SMAP / UMIP | true | **false** |
| `[alt]` patch sites applied | 47 of 47 | **0 of 47** |
| everything else (SSE4, POPCNT, AVX, XSAVE, AES-NI, RDRAND, RDTSCP, 1 GiB pages) | absent | absent |

The cause is not WHPX's nature but our command line: `boot-test.sh` passes
`-cpu qemu64,+smep,+smap,+umip`, and under WHPX the three `+` augmentations are
silently dropped — the base model is otherwise identical, which is why every
other feature matches exactly.

**The runtime consequence is bounded and it is knowable, not a vague caveat.**
`alternatives.rs` has exactly one `Feature` variant, `Smap`, so all 47 sites are
`stac`/`clac` insertions on kernel paths that touch user memory. SMEP and UMIP
add no instructions at all — they are CR4 bits that turn certain accesses into
faults. So the entire executed-code difference is: under TCG, user-memory
accesses carry a `stac`/`clac` pair that under WHPX is absent.

That confounds the user-copy and syscall-boundary benchmarks by an unknown but
small amount, and leaves the headline numbers standing, because the largest
effects are on paths that never touch user memory: `ipc_channel_roundtrip_64k`
(×10.36), `sched_pick_next_d1`/`d256` (×8.20/×8.40), `io_ring_nop` (×9.57),
`crypto_x25519` (×8.27). Two emulated instructions cannot produce ×8 on a
benchmark that does not execute them.

**It does not weaken the decision below; it strengthens it.** The grouping key
was adopted because a cross-accelerator comparison is meaningless. This shows the
accelerator drags *other* machine properties along with it, so the records being
separated differ by even more than the argument assumed.

### Why the value is read from the guest, not from the flag we passed

The harness knows perfectly well which accelerator it asked for — it is on its
own command line. Recording *that* would have been one line and would have been
wrong. A flag records an intention; the boot banner records an outcome, and they
come apart in exactly the cases that matter: a stale build that never got the
flag, a mistyped `QEMU_EXTRA`, a changed default, and above all a **silent
fallback** — QEMU asked for an accelerator it cannot provide will happily fall
back to TCG. In every one of those the harness would confidently write "WHPX"
onto a TCG run, which is worse than the gap it was meant to close: an absent
field is at least honest about not knowing.

The kernel had been printing the answer on every boot since the hypervisor
detection work (`[hypervisor] Detected: … (signature: …)`, from the guest's own
CPUID), so this is not new instrumentation. It is the harness ceasing to discard
something it was already being told.

### Why it is part of the grouping key rather than a check

The alternative was to keep grouping by commit and add a validation step — "if
the records in this group disagree about the accelerator, refuse". That works
until someone adds a second path that builds a group, or refactors the one that
exists, and forgets the check; then the check is absent and nothing says so.
Putting the accelerator in the key makes a mixed group *unconstructible*: there
is no arrangement of records, and no future code path, that can produce one,
because such records are no longer the same key.

The stakes justify the stronger form. A layout band spanning both accelerators
would report ×10 as "how far code placement alone can move this benchmark" — a
band near 1000%, which then silently excuses every regression it is ever
consulted about. That is the single worst failure this subsystem has: not a
false alarm, but a permanent, invisible all-clear.

### Absent is its own value and is never folded into TCG

The same rule §234 set for `text_pad`, and here it is not an analogy but a
proof. The first WHPX run in our history was recorded *before* this field
existed, so it carries no accelerator — an absent value is demonstrably not
evidence of TCG, because at least one absent value is WHPX. Defaulting absent to
TCG would have enrolled every one of the ~97 pre-field records into an arm they
were never part of. Absent therefore gets its own key: historical records keep
forming exactly the bands they always did, and never mix with a record that
actually said.

### The cost, and what is not decided here

Bands and baselines now fragment by accelerator, so switching accelerators
invalidates every band we have measured and requires a fresh 69-minute sweep
before placement can excuse anything again. That is the correct cost — the
alternative is bands that quietly mean nothing — but it does mean the
accelerator choice is now a heavier decision than it looks.

**This entry does not decide which accelerator we should use.** That is a
cross-lane environment change (`known-issues.md` explicitly fences
`scripts/boot-test.sh`'s accelerator as shared by all three lanes) and it trades
a measurement artifact against real lost coverage — WHPX cannot expose UMIP on
this host, so our SMEP/SMAP/UMIP hardening would go untested. It is filed for
the operator, not settled here.
