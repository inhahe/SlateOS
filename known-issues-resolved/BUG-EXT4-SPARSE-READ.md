### BUG-EXT4-SPARSE-READ. ext4 extent reader collapsed sparse holes — every sparse file read back corrupted (data shifted left into the holes) — 2026-07-23 — ✅ RESOLVED 2026-07-23

**Where:** `kernel/src/fs/ext4/driver.rs` — `read_range_from_tree` (the page-cache
fill primitive behind `read_at`/`mmap`), `read_extent_data` + `read_extent_tree_recursive`
(the full-file `read_file_data` path).

**What:** All three extent readers assembled file data by **appending** each
allocated block's bytes to the output (`result.extend_from_slice(...)`, tracking
position by `result.len()`), and **never consulted `extent.ee_block`** (the block's
logical offset). For a *contiguous* file this is fine. For a **sparse** file (one
with unmapped logical blocks — holes), the gaps were simply skipped, so every block
after a hole was placed at the wrong offset — the data after each hole got dragged
left into the hole. The read came back the correct *length* only because the read
path pre-zeroes a full-size page-cache buffer (`read_file_routed` → `read_through`),
and the *zero-count* happened to match — but the bytes were permuted, so any hash
differed.

**How it manifested:** the fastpy self-test ELFs (moved onto the rootfs disk for
TD-KERNEL-EMBED-BLOAT) are **>50 % zeros** and stored sparse by `cp`/mkfs (e.g.
`fastpy-hello.elf`: 3 475 528 bytes, 1 777 808 zero, ~214 hole blocks). Loaded from
`/mnt/tests`, every one validated as an ELF (header/TLS in the first, hole-free
blocks) and reached `Zombie`, then **jumped through a null function pointer**
(`rip=0x0`, exit code -8) because its `.data`/relocated code had been shifted — 100 %
of fastpy ring-3 tests failed identically. Dense files (glibc `.so`) were unaffected,
which is why real-glibc Path-Z tests always passed. Confirmed by checksumming the
loaded ELF in-kernel: FNV `0x42eb…bf12` vs host `0x77f4…bf12` (same len, same zero
count), then byte-exact match after the fix.

**Fix:** rewrote all three readers to place each allocated block at its **absolute
logical offset** in a pre-zeroed output buffer, leaving holes and unwritten extents
as zeros. Extracted the placement arithmetic into a pure `block_copy_placement()`
helper (unit-tested in `#[cfg(test)] mod placement_tests` and by the boot-time
`test_block_copy_placement` in `driver::self_test`). Verified: all 55 sparse fastpy
ring-3 self-tests pass, byte-exact ELF load, green boot.
