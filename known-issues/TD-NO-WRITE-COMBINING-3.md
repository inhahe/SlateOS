### TD-NO-WRITE-COMBINING -- UPDATE 2026-08-17 (second) -- the ratio is measured; it is **not measurable** under QEMU TCG, and the acceptance criterion stated just above is unsound

**In short:** the fill-ratio measurement the previous update called for now
exists and has run. It reported **0.94x** -- write-combining fractionally
*slower* than uncached. That is not the bug it looks like: the test platform is
QEMU's TCG interpreter, which emulates no cache and no store buffer at all, so a
write-combining mapping and an uncached one run the identical host code and the
ratio is pinned near 1.0 no matter how correct the kernel is. The paragraph above
is therefore wrong where it says a ratio near 1.0 is "unambiguous evidence that
slot 1 is not being selected". It is only evidence of that on a platform that
models memory types in the first place.

**What was built.** `VramAperture::measure_write_combining`
(`kernel/src/drm/ati/aperture.rs`) fills the same 1 MiB of the same BAR twice in
one boot -- once with the PTEs saying `WC`, once saying `UC` -- and
`drm::ati::report_write_combining` reports the ratio. Two ways to get a
flattering number are defended against explicitly: each timed region ends with
`sfence` *inside* the clock, so WC is not credited for stores still sitting in a
fill buffer; and each half gets identical preparation, so neither pays for work
the other avoids.

**Measured, boot #12 (PASS, 1244 s):**

```
[ati]   VRAM aperture mapped: 16 MiB at 0x80000000 (memory type WC, writes may linger: false)
[ati]   Write-combining measured over 1024 KiB: WC 810133928 cycles vs UC 764480631 cycles = 0.94x
```

**Why that is a property of the emulator, not of the kernel.** 810133928 cycles
over 262144 four-byte stores is ~3090 cycles per store -- orders of magnitude
above any real memory or MMIO write, and squarely in
trap-to-the-device-model territory. `scripts/boot-test.sh` passes no `-accel`
flag, so QEMU runs TCG. TCG interprets or JITs every guest store and models no
store buffer, no cache and no memory types whatsoever; `WC` and `UC` reach the
same host code. The 6% difference is noise.

The kernel already had this information and did not use it: `[hypervisor]
Detected: QEMU TCG (signature: "TCGTCGTCGTCG")` is printed at serial line **71**,
about 1600 lines before the ATI probe warned at line 1677.

**What landed as a result.**

* `Hypervisor::models_memory_types()` (`kernel/src/hypervisor.rs`) -- false only
  for `QemuTcg`. Every other hypervisor runs guest page tables on the real MMU,
  so guest `IA32_PAT` governs real caching there and a low ratio *would* be a
  genuine finding. `Unknown` answers `true` deliberately: the failure that
  matters is suppressing a real finding, not printing an inconclusive one.
* The verdict in `report_write_combining` branches on it. Under TCG it reports
  the number as *not measurable* and says so; elsewhere it still warns.
* **The warning stopped blaming the wrong thing.** It used to end "check that PAT
  slot 1 is selected" -- which the line printed immediately above it already
  disproves, since that line decodes the mapping's live memory type as `WC`. A
  diagnostic that sends its reader to re-verify the one thing already proven is
  worse than none. It now names the actual remaining suspect on real hardware: an
  MTRR covering the aperture that says `UC` overrides PAT, because the effective
  type is the *combination* of the two and for MTRR=UC the combination is UC
  regardless of PAT (Intel SDM Vol. 3, Table 11-7). That is how a
  correctly-programmed WC mapping still ends up uncached on metal.

**A methodology bug found and fixed while doing this.** The first version warmed
the TLB once, before the WC pass only. But switching the range to `UC` runs
`invlpg` on every hardware page and one `wbinvd`, so the UC pass then started
with a cold TLB and cold caches -- it was charged for refills the WC pass had
been spared. That biases the ratio *upward*: the measurement would have
flattered write-combining by handicapping its rival, which is exactly the class
of error the function's own doc comment claims to defend against. Both halves now
get the same remap-then-warm-up sequence, including a redundant remap-to-WC
before the WC half.

**Still open -- and now blocked on a platform rather than on work.** Everything
that can be verified without hardware that models memory types has been: the MSR
reads back as programmed, the four named `PageFlags` constants decode to
`WB`/`WC`/`WT`/`UC` through the table actually in force, no live mapping changed
meaning (`design-decisions.md` §219), and the measurement harness itself is
sound. What remains is one number from a platform capable of producing it:

* Real hardware with an ATI RV100 -- adjacent to `open-questions.md` Q49.
* Or QEMU with `-accel whpx` (this is a Windows host) / KVM. **Unverified, and
  not automatically sufficient:** hardware-accelerated guests honour guest PAT
  for memory the host maps into the guest, but only if QEMU's `ati-vga` BAR0 is a
  RAM-backed memory region rather than a trapping device model. The ~3090
  cycles/store above is weak evidence it may be the latter, in which case
  acceleration will not help and no QEMU configuration can measure this.
  Worth one manual QEMU run to find out; do **not** change
  `scripts/boot-test.sh`'s accelerator to test it, since that would alter the
  test environment for all three lanes.

Until then this entry stays open, and the guard against silently regressing to
uncached is the WC-vs-UC line itself: on any platform that models memory types it
warns, and on TCG it says plainly that it cannot tell.

**2026-08-19 -- the manual QEMU run happened, and write-combining works.** The
run was made the sanctioned way, via `QEMU_EXTRA` on a one-off `--experiment`
boot; `scripts/boot-test.sh`'s accelerator is untouched and all three lanes still
run TCG. Each log states its own accelerator from the guest's CPUID, so the
pairing below is not an assumption but a reading:

| Accelerator | WC cycles | UC cycles | Ratio |
|---|---|---|---|
| QEMU TCG | 360,219,423 | 369,748,072 | **1.02x** |
| Hyper-V/WHPX | 636,766 | 32,948,353 | **51.74x** |

**The doubt above is resolved in the favourable direction: QEMU's `ati-vga` BAR0
is a RAM-backed memory region, not a trapping device model.** Had it been the
latter, every store would have trapped to the hypervisor and the ratio would have
stayed ~1x under WHPX too. It did not -- WC is 51.74x faster than UC, which is
only possible if the CPU is genuinely combining stores in a write-combining
buffer, which in turn is only possible if the guest PAT is being honoured by
hardware.

The per-store costs settle it beyond the ratio. The fill is 4-byte (`u32`)
stores over 1 MiB, so 256 Ki stores per pass:

| | cycles/store | reading |
|---|---|---|
| WHPX, WC | **2.4** | stores absorbed by the write-combining buffer, retiring at near register speed |
| WHPX, UC | **125.7** | each store going to the bus on its own, as `UC` requires |
| TCG, WC | 1374 | emulation overhead |
| TCG, UC | 1410 | the same emulation overhead |

Both WHPX figures are what the hardware ought to produce, and they are what a
trapping device model could not: a VM exit costs ~13.5 us (see
`design-decisions.md` §237, measured on this host), which is thousands of cycles,
so 2.4 cycles/store rules out a trap per store outright. The earlier worry that
"~3090 cycles/store is weak evidence of a trapping model" was reading TCG's
per-store *emulation* overhead -- which swamps the memory type entirely, and is
exactly why TCG's two halves come out 1374 and 1410, i.e. 1.02x.

**What this does and does not establish.** It establishes that our PAT
programming is correct end to end -- the MSR we write, the `PageFlags` bit
pattern we choose, and the mapping we install really do produce write-combining
on hardware that implements it. That was the actual open question, and it is
answered. It does not establish anything about a *real* RV100's aperture, whose
behaviour also depends on the card's own memory controller; but that was always
a hardware question (Q49), and it is no longer blocking confidence in the kernel
side.

**Status: the kernel-side question is closed.** The entry stays open only for
the real-hardware confirmation, and the standing guard is unchanged: on a
platform that models memory types the WC-vs-UC line warns, and on TCG it says
plainly that it cannot tell. Note the corollary for anyone reading a TCG boot
log: a 1.02x reading there is *not* a regression report, it is the absence of a
measurement.

**Provenance, and a mistake worth not repeating.** The figures above were read
from the two boots' serial logs and transcribed here; the WHPX log itself is
gone. `build/serial-test.txt` is gitignored scratch that `boot-test.sh` deletes
at the start of every boot, and a later WHPX run in the same session overwrote
it before it had been copied anywhere. This file already warns about exactly that
(see the `boot-history.jsonl` entry above: "gitignored scratch that the next run
overwrites, so until now the evidence for a hang survived only if somebody pasted
it in here before the next boot", which had already cost the
`B-FORKEXEC-BOOT-HANG` investigation) -- and the warning was not heeded a second
time. Nothing was actually lost here, because the numbers were transcribed before
the overwrite and the benchmark half is committed in `bench/history.jsonl`; but
the near-miss makes the rule concrete: **an experimental boot's log is evidence
only once it has been copied out from under `build/`, and that copy must happen
in the same step that reads it, not later.** `build/serial-canary-fail1.txt` (the
TCG side) survived precisely because it had been given a stable name.
