### B-LIMINE-RSDP-REQUEST-CARRIED-THE-WRONG-FEATURE-ID — 2026-08-13 — ✅ FIXED

**What.** `kernel/src/limine.rs` declared the ACPI RSDP request with feature ID
`[0x71ba_7686_3cc5_5f63, 0xb264_4a48_c516_a487]`. That is
`LIMINE_EXECUTABLE_ADDRESS_REQUEST` (`limine/limine.h:648`), not
`LIMINE_RSDP_REQUEST` (`limine.h:555`, `[0xc5e7_7b6b_397e_7b43,
0x2763_7845_accd_cf3c]`).

**Why it hid for so long.** It did not fail — it *succeeded at the wrong
thing*. Limine happily answered the request it was actually asked, so
`RSDP_REQUEST.response()` returned non-null and `RsdpResponse { revision,
address }` overlaid `limine_executable_address_response { revision,
physical_base, virtual_base }`. `address` therefore read back as the kernel's
physical load address. `acpi::init` checked the `"RSD PTR "` signature, found
none, printed one line, and fell back to brute-force scanning ACPI-reclaimable
memory — which finds the real RSDP under QEMU/SeaBIOS. Every boot log carried
the evidence:

```
[boot] RSDP address from Limine: 0x74c43000
[acpi] RSDP not found at provided address 0x74c43000 (tried virt=0xffff800074c43000)
[acpi] Limine RSDP address invalid — scanning memory...
[acpi] RSDP found at phys=0x7f77e000
```

…and a comment in `acpi::init` had even rationalised it as a bootloader quirk
("observed on QEMU+edk2 where Limine returns the kernel load address instead").
It was our bug, not Limine's.

**Impact — not hypothetical; measured.** The scan is a heuristic the RSDP
request exists to avoid, and it was picking the *wrong table*. QEMU publishes
two RSDPs 20 bytes apart; scanning on 16-byte boundaries hits the ACPI 1.0 one
first and stops. So every boot took the legacy path:

| | RSDP | revision | root table |
|---|---|---|---|
| before (scan) | `0x7f77e000` | 0 — ACPI 1.0 | **RSDT** `0x7f77d000` (32-bit pointers) |
| after (bootloader) | `0x7f77e014` | 2 — ACPI 2.0+ | **XSDT** `0x7f77d0e8` (64-bit pointers) |

Both enumerate the same 6 tables on this machine, so nothing was visibly
broken — but the RSDT physically cannot address a table above 4 GiB, and
`init()`'s "prefer XSDT" branch had never once been taken. On UEFI the RSDP
also need not be in either scanned region at all.

**Fix.** Corrected the RSDP ID and added a properly-typed
`ExecutableAddressResponse` request under the ID that was being misused —
which `alternatives::apply()` now needs anyway, to find `.text`'s physical
pages.

**Lesson.** A magic constant that names the *wrong* feature is invisible to
every test that only asks "did we get an answer?". The two-line diff that
would have caught it is checking the ID against `limine/limine.h`, which is
vendored in this repo.
