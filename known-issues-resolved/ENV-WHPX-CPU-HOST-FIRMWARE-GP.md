### ENV-WHPX-CPU-HOST-FIRMWARE-GP -- `-accel whpx` cannot be combined with `-cpu host`: OVMF #GPs in `PlatformPei` before the kernel loads -- 2026-08-19 -- CLOSED as a fact of the environment (no fix available to us; recorded so it is not re-attempted)

**Why this was tried.** Under `-accel whpx`, the guest reports **no** SMEP, SMAP
or UMIP, while under TCG it reports all three. The cause is not WHPX's nature but
our command line: `scripts/boot-test.sh` passes
`-cpu qemu64,+smep,+smap,+umip`, and under WHPX the three `+` augmentations are
silently dropped. Every other feature matches exactly between the two
accelerators (no SSSE3, SSE4.x, POPCNT, AVX, XSAVE, AES-NI, RDRAND, RDTSCP or
1 GiB pages in either), which is what identifies the cause as the augmentations
specifically rather than a different base model.

The obvious repair is `-cpu host`, which under a hardware accelerator ought to
hand the guest the real CPU's features -- recovering SMEP/SMAP/UMIP and, as a
bonus, giving us the first boot we would ever have run on a CPU advertising
XSAVE/AVX/AES-NI, all of which `qemu64` hides.

**It does not work.** `QEMU_CPU=host QEMU_EXTRA="-accel whpx"` never reaches our
kernel. OVMF dies in early platform init:

```
!!!! X64 Exception Type - 0D(#GP - General Protection)  CPU Apic ID - 00000000 !!!!
RIP  - 0000000000834EEE, CS  - 0000000000000038, RFLAGS - 0000000000000046
RAX  - 0000000000000005, RCX - 000000000000003A
!!!! Find image based on IP(0x834EEE) ... OvmfPkg/PlatformPei/PlatformPei/DEBUG/PlatformPei.dll !!!!
```

Log preserved at `build/serial-whpx-cpuhost-gp.txt` (copied out immediately --
see the provenance note on the write-combining entry for why that matters).

`RCX` holds the MSR index for `rdmsr`/`wrmsr`, and `0x3A` is
`IA32_FEATURE_CONTROL` -- the VMX/SMX lock register. `RAX = 5` is that register's
lock bit plus "VMX outside SMX". So this is consistent with OVMF's platform init
touching `IA32_FEATURE_CONTROL` on a CPU whose CPUID (now the host's, thanks to
`-cpu host`) advertises VMX, on an accelerator that does not implement the MSR --
WHPX exposes no nested virtualization. Under `qemu64` the CPUID never advertises
VMX, so the code path is never taken, which is why only `-cpu host` trips it.

**Consequence, and why it matters beyond curiosity.** There is no QEMU
configuration on this host that gives us WHPX *and* SMEP/SMAP/UMIP:

| | TCG | WHPX + `qemu64` | WHPX + `host` |
|---|---|---|---|
| Boots | yes | yes | **no -- firmware #GP** |
| SMEP/SMAP/UMIP | yes | **no** | n/a |
| `[alt]` sites patched | 47/47 | **0/47** | n/a |

So the accelerator choice is a genuine either/or: TCG buys the hardening
coverage (and the 47 `stac`/`clac` alternative sites, `alternatives.rs` having
exactly one `Feature` variant, `Smap`), WHPX buys ~3.5x faster benchmarks and
the ability to measure anything that depends on real memory types. That is what
makes it an operator decision rather than an obvious one.

**Not worth re-attempting without new information.** Intermediate `-cpu` models
were not swept, and one might in principle boot while carrying SMEP/SMAP/UMIP --
but the augmentation-dropping above suggests WHPX ignores `+feature` regardless
of base model, so the likely outcome is either `qemu64`'s feature set or
`host`'s crash. If someone does try, the cheap probe is the `[cpu] Feature
detection:` block in the first 80 lines of serial: it answers in one boot.
