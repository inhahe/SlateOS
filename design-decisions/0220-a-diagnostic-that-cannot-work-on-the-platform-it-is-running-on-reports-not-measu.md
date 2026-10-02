## §220 — A diagnostic that cannot work on the platform it is running on reports "not measurable", never "failed"

**Date:** 2026-08-17
**Decided by:** Claude (autonomous)
**Lane:** C

**In short:** The kernel now measures whether write-combining — a CPU feature
that batches many small writes to the graphics card into a few large ones — is
actually making writes to the card's memory faster. Under QEMU's software
emulator, which is how this whole project is tested, the answer is always "no
faster", because the emulator does not pretend to have a cache or a write buffer
at all. The choice was between printing a warning anyway (it looks exactly like a
real fault) and detecting the emulator and saying plainly that the number means
nothing there. We detect the emulator. The cost accepted is that on the emulator
this particular test can no longer catch a genuine write-combining bug — but it
never could; it only looked as though it might.

**The decision.** `drm::ati::report_write_combining` branches its verdict on
`hypervisor::Hypervisor::models_memory_types()`, which is false for `QemuTcg`
alone. On TCG a low ratio is reported as *not measurable*, with the reason. On
anything else it is reported as a `WARNING`, because there a low ratio means
something.

The underlying asymmetry is not about virtualization but about who owns the page
tables. A hardware-virtualized guest — KVM, Hyper-V/WHPX, VMware, VirtualBox,
Xen, bhyve — runs its own page tables on the real MMU, so the guest's `IA32_PAT`
governs real caching and write-combining genuinely happens or genuinely does not.
QEMU's TCG interprets or JITs every guest store into host code that models no
store buffer, no cache and no memory types; a `WC` mapping and a `UC` mapping
execute the *same* host instructions. Measuring a memory type there is not
failing, it is unmeasurable.

**Alternatives considered.**

1. **Warn unconditionally.** Rejected. It fires on every single boot of the only
   platform this project is tested on. A warning that always fires is one its
   reader learns to skip, which spends the credibility of the line on the one
   occasion it would have mattered. This is not hypothetical for us: the
   contradictory-and-therefore-useless stall dump in
   `known-issues.md` → `BUG-BOOT-SPINLOCK-STALL-UNNAMED` cost real debugging time
   for exactly this reason.
2. **Delete the measurement.** Rejected. It is the only check that can catch this
   failure on real hardware, and the failure is silent by nature: a wrong PAT slot
   index yields a *different valid memory type* rather than a fault, and `UC` and
   `WC` are indistinguishable to every decode-side check the driver can make (both
   mean "writes do not linger"). Speed is the only evidence that exists.
3. **Gate on `is_virtual()` rather than on TCG specifically.** Rejected, and this
   is the alternative worth naming: it would have suppressed the warning under
   KVM and WHPX too, which is where we are most likely to ever catch this bug for
   real. The gate must be as narrow as the actual limitation.
4. **Fail the boot on a low ratio.** Rejected. An uncached framebuffer is correct,
   merely slow; refusing to boot would trade a working slow display for no
   display.

**Consequences.**

1. On TCG the line is informational and the aperture's correctness rests on
   `mm::pat::self_test` instead, which verifies the MSR contents and that each
   named `PageFlags` constant decodes to the intended memory type through the
   table actually in force. That is a weaker guarantee than the ratio and is
   documented as such in `known-issues.md` → `TD-NO-WRITE-COMBINING`, which stays
   open pending a platform that can produce the number.
2. `Hypervisor::Unknown` answers `true` — an unrecognised signature warns rather
   than staying silent. The failure mode worth avoiding is suppressing a real
   finding, not printing an inconclusive one.
3. The general rule this sets for kernel diagnostics: **prefer an honest "cannot
   tell" to a false "failed", and equally to a false "passed".** A check should
   know the conditions under which it is capable of concluding anything, and say
   when they do not hold. The same principle produced the companion change in
   `lockdep::dump_held_locks`, which now prints "validator DISABLED" instead of a
   confident "0 lock(s)" it has not earned.
4. A corollary applied at the same time: **a diagnostic must not accuse something
   the log has already cleared.** The old warning ended "check that PAT slot 1 is
   selected" while the line printed immediately above it decoded the mapping's
   live memory type as `WC`. It now names the suspect that is actually still
   open — an MTRR covering the range that says `UC`, which beats PAT because the
   effective type is the combination of the two (Intel SDM Vol. 3, Table 11-7).
