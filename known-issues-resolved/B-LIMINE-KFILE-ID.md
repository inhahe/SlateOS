### B-LIMINE-KFILE-ID. Wrong Limine kernel-file request feature-ID → boot cmdline AND kernel-file symbolization silently never worked — FIXED 2026-07-14

**Where:** `kernel/src/limine.rs`, `LimineRequest::<KernelFileResponse>::KERNEL_FILE`.

**Bug:** the request's second feature-id word was `0x31eb_5d10_c871_c930`, which
does not match Limine's `LIMINE_{KERNEL,EXECUTABLE}_FILE_REQUEST` magic — the
correct value (per `limine/limine.h`, Limine 8.7.0) is `0x31eb_5d1c_5ff2_3b69`.
Because the ID never matched, Limine never populated the response, so
`boot::kernel_cmdline()` always returned `None` and `boot::kernel_file_address()`
(used for panic/backtrace symbolization from the kernel ELF) always returned
`None`. Two silent consequences: (1) the boot command line was invisible to the
kernel — `fs::kernparam` saw an empty cmdline regardless of what the bootloader
passed, so cmdline-gated switches (e.g. `net.userspace`) could never be turned
on at runtime; (2) kernel-file-based symbolization was inert. **Fix:** corrected
the feature-id word. Verified: `cmdline: net.userspace` in `limine.conf` now
round-trips into `kernparam` and flips the cutover switch. **Repro (pre-fix):**
add any `cmdline:` to `limine.conf`; the kernel read it as empty.
