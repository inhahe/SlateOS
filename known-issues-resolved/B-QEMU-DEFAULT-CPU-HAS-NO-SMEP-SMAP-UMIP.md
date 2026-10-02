### B-QEMU-DEFAULT-CPU-HAS-NO-SMEP-SMAP-UMIP. The boot test never exercises the supervisor-mode protections — 2026-08-12 — ✅ FIXED 2026-08-13 (`scripts/boot-test.sh`)

**What.** The boot log shows all three protections unavailable, so the code paths
that set CR4.SMEP/SMAP/UMIP never execute under test:

```
[smep_smap] SMEP not supported by CPU
[smep_smap] SMAP not supported by CPU
[smep_smap] UMIP not supported by CPU
[smep_smap]   CR4=0x620
```

QEMU's default CPU model (`qemu64`) does not advertise these features. So
`smep_smap`'s enable paths, and the `stac()`/`clac()` bodies (skipped in the
self-test because the instructions would #UD), are **entirely untested** —
including on the one machine that runs our whole test suite. **SMEP in particular
is assumed to be protecting us and is in fact inactive**, on hardware and in CI
alike, whenever CPUID does not advertise it.

**Fixed by** adding `-cpu "$QEMU_CPU"` to the QEMU invocation in
`scripts/boot-test.sh`, defaulting to `qemu64,+smep,+smap,+umip` and overridable
via the `QEMU_CPU` environment variable. The kernel boots to `BOOT_OK` (262 s)
with SMEP and UMIP genuinely enforced for the first time:

```
[smep_smap] Enabling SMEP (kernel exec of user pages blocked)
[smep_smap] SMAP supported (enablement deferred — IDT entry stubs do not clear
            EFLAGS.AC (B-AC-INHERITED-AT-KERNEL-ENTRY))
[smep_smap] Enabling UMIP (user SGDT/SIDT/SLDT/SMSW/STR blocked)
[smep_smap] CR4 updated: 0x20 → 0x100820
...
[smep_smap]   Active: SMEP=true, SMAP=false, UMIP=true    CR4=0x100e20
[smep_smap]   SMEP enforcement: VERIFIED (CR4 bit set)
[smep_smap]   UMIP enforcement: VERIFIED (CR4 bit set)
[smep_smap]   STAC/CLAC pair: OK (no fault)
```

Two results worth noting: the kernel boots cleanly with SMEP active, i.e. no
kernel code path executes from a user page; and `stac()`/`clac()` executed for
the first time ever (previously skipped as they would #UD), so that
infrastructure is now covered rather than merely written.

**Where.** `scripts/boot-test.sh` (`QEMU_CPU`); `kernel/src/smep_smap.rs`
(the formerly-untested paths).
