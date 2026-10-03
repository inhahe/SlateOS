### B-ACPI-PROBED-THE-BOOTLOADER-RSDP-WITHOUT-MAPPING-IT — 2026-08-13 — ✅ FIXED

**What.** `acpi::try_rsdp_address()` dereferenced the bootloader-supplied RSDP
address through `check_rsdp_signature()` without first ensuring the page was
present in the HHDM. `check_rsdp_signature`'s own SAFETY comment stated the
obligation — "caller must ensure the virtual address is mapped" — and this
caller did not meet it. Its sibling `scan_for_rsdp()` had always called
`ensure_hhdm_mapped()` first.

**Why it hid for so long.** It was masked by
`B-LIMINE-RSDP-REQUEST-CARRIED-THE-WRONG-FEATURE-ID` above. While the
"RSDP address" was really the kernel's load address, the probe read from
inside the kernel image, which is always mapped — so it returned `false`
harmlessly. Fixing the request ID made the address point at ACPI-reclaimable
memory, which Limine's HHDM does not necessarily cover, and the very next boot
died:

```
EXCEPTION: Page Fault (#PF) at 0xffffffff81ecd8ba, address=0xffff80007f77e014, error=0x0
  Cause: not-present, read, kernel
FATAL: Unrecoverable kernel page fault. Halting.
```

**Fix.** `try_rsdp_address` now maps the candidate (`ensure_hhdm_mapped`, full
36-byte ACPI 2.0 extent) before probing it, and only after checking the range
lies inside a memory-map entry — so a bad address cannot cause a mapping over
MMIO or past the end of RAM, where the probe could hang real hardware rather
than merely failing the signature test. `check_rsdp_signature` and
`scan_for_rsdp` were made `unsafe fn`, so the mapping obligation is now
enforced by the compiler instead of by a comment. The low-memory scan branch,
which also relied on "usable memory is normally mapped", now maps explicitly
too.

**Lesson.** Two bugs cancelling out is worse than either alone: the wrong
request ID was *protecting* the unmapped read, so fixing one exposed the
other. When a SAFETY comment states an obligation that the function's own
signature does not enforce, expect at least one caller to be violating it.
