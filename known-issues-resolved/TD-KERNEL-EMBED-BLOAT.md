### TD-KERNEL-EMBED-BLOAT. Kernel image grows ~3.5 MiB per fastpy self-test — every self-test ELF is baked into `.rodata` via `include_bytes!` — 2026-07-22 — ✅ RESOLVED 2026-07-23 (Q33-B: staged onto rootfs disk, loaded at runtime)

**What:** Each fastpy ring-3 self-test embeds its native SlateOS ELF directly
into the kernel binary with `include_bytes!` (see the ~47 embeds under
`kernel/src/proc/spawn.rs`, paths `../../../services/fastpy-*/fastpy-*.elf`).
Each ELF is ~3.5 MiB (fastpy statically links its whole runtime + libc into
every binary), so the kernel's `.rodata` is now ~165 MiB and the staged
(stripped) kernel image is ~202 MiB. Limine must load the whole image into
high memory at boot; with QEMU's old `-m 512M` the setuid self-test (the 47th
embed) tipped the loader past its budget → Limine `PANIC: High memory
allocator: Out of memory` before the kernel ever ran.

**Repro (historical):** add one more fastpy self-test around the 512 MiB edge,
boot-test with `-m 512M` → Limine OOM panic during `/boot/kernel` load.

**Mitigation applied:** raised the boot-test QEMU RAM to `-m 3072M`
(`scripts/boot-test.sh`). This is realistic for a desktop OS and gives ample
headroom, but it only *defers* the problem — the image keeps growing ~3.5 MiB
per new self-test.

**Proper fix:** stop baking self-test binaries into the kernel image. Stage the
fastpy `*.elf` files onto the ESP (or the ext4 rootfs) at boot-test time and
have the kernel self-tests load them from disk via the VFS, exactly like a real
`spawn` from a file. That keeps the kernel image small (only real kernel code),
removes the per-test `.rodata` growth, and makes the self-tests exercise the
disk/loader path too. Secondary lever: shrink each fastpy ELF (shared runtime
`.so` / dead-strip) so even embedded builds are smaller.

**Resolution (2026-07-23, Q33-B — see design-decisions.md #86):** implemented the
proper fix. The 54 `include_bytes!` fastpy embeds in `kernel/src/proc/spawn.rs` were
converted to runtime disk loads via a new `load_test_elf(name)` helper that reads
`/mnt/tests/<name>.elf` through `crate::fs::Vfs::read_file` (the ext4 rootfs is
mounted at `/mnt` before the Path-Z self-tests run). Each test self-*skips*
(`return Ok(())`) when the fixture is absent, so a lean build with no test disk still
boots green. `scripts/create-ext4-rootfs.sh` stages all fastpy `*.elf` into `/tests`
and the image grew 48M→256M to hold them. **Result: debug kernel binary 361.7 MiB →
181.8 MiB (−180 MiB / ~50 %).** Uncovered and fixed BUG-EXT4-SPARSE-READ in the
process (the sparse fastpy ELFs were the first sparse files ever read through the
extent path). Verified: 55/55 fastpy ring-3 self-tests pass loading from disk, green
boot. Follow-on secondary lever (shared-runtime `.so` to shrink each ELF) remains a
possible future optimization but is no longer urgent.
