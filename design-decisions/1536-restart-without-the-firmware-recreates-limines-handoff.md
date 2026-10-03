## 1536. A restart without the firmware recreates Limine's handoff for the new kernel

**Date:** 2026-10-03 · **Decided by:** Claude (operator-approved scope: the
whole-kernel swap of §1126, the operator's decision, and `power.reload` in
`roadmap-detailed.md` §1.5) · **Lane:** A

**In short:** "Restart OS -- keep the computer on" (lane C's start menu, lane
B's `powerctl reload`) needs the running kernel to start a kernel image
itself, without going back through the firmware: load the image, stop the
machine's other activity, and jump in. The new kernel is started exactly as
Limine starts it -- the running kernel writes the same six answers Limine
would (the memory map and the rest) into the new image and builds the page
tables Limine would -- so the new kernel boots through its one, ordinary
boot path and does not know it was not started by the firmware. This is the
load-and-jump half of §1126's live kernel replacement; the process records
come later, on the same mechanism.

**What the kernel needs from Limine** (`kernel/src/boot.rs`): base revision
3, the memory map, the HHDM offset, the framebuffer, the RSDP, its own load
address, and its own ELF file. It starts its other CPUs itself (INIT-SIPI),
asks for no modules (the services it starts are embedded), and builds its
own page tables early. So the handoff to recreate is small.

**The mechanism:**

1. **Load.** Parse the new image's `PT_LOAD` segments, copy them into one
   physically contiguous run of frames (the kernel converts its own virtual
   addresses by subtracting a single base), zero the BSS.
2. **Answer the requests.** Find the Limine request blocks in the loaded image
   (their 32-byte ids, between the start and end markers), build each
   response in memory reserved for the handoff, and write its address into
   the request -- as Limine does. Mark the base revision supported. The
   memory map is the *firmware's*, as the first boot received it (kept from
   boot for this), with the new image marked kernel-and-modules and the
   handoff (responses, page tables, stack) bootloader-reclaimable; the old
   kernel's own memory reads as usable, which it is once the jump is made.
3. **Page tables** as Limine builds them: the image at its linked addresses,
   and every physical range at the HHDM offset -- the *same* offset the
   running kernel uses, so the trampoline that switches tables runs at one
   address valid in both.
4. **Quiesce.** Interrupts off; every IOAPIC entry masked and the LAPIC timer
   stopped; **Bus Master Enable cleared on every PCI function**, which stops
   all device DMA at once without a shutdown routine per driver (each driver
   resets its device when the new kernel probes it); the IOMMU's translation
   off, so the new kernel's first DMA is not translated through the old
   kernel's tables; every other CPU sent INIT, which leaves it waiting for a
   SIPI exactly as the firmware does.
5. **Jump.** A trampoline copied into the handoff memory loads a minimal GDT
   and an empty IDT, switches to the new tables and stack, and enters the
   new image's entry point -- interrupts off, as Limine leaves them.

**The alternative weighed:**

| | For | Against |
|---|---|---|
| **Recreate Limine's handoff (chosen)** | the new kernel boots through the path every boot exercises; it needs no second entry point and no knowledge of being restarted; any Limine-protocol image works, an older SlateOS included | the running kernel must reproduce Limine's structures faithfully -- but only the six the kernel asks for |
| A SlateOS hand-over entry taking a boot-facts record | the record is ours to version, and §1126's process records will need a hand-over entry anyway | a second boot path in every kernel, exercised only by restarts; every consumer of boot information has to read it through an abstraction |

§1126's full swap still needs its hand-over entry for the process records;
that entry will take the boot facts the same way this one does, so the two
share the loader, the page tables, the quiesce and the trampoline.

**Gate:** `power.reload`, distinct from `power.reboot` because the caller
chooses the image (lane C's request): a capability of its own.

**Revisit if:** the kernel starts asking Limine for more (modules, SMP), each
of which the handoff must then recreate too.
