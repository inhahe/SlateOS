### B-FALLOC1. Linux-ABI `fallocate` COLLAPSE_RANGE/INSERT_RANGE now shift contents; only UNSHARE_RANGE still EOPNOTSUPP — PARTIALLY RESOLVED 2026-06-18, COLLAPSE/INSERT ADDED 2026-06-20

**Status:** `sys_fallocate` (syscall #285) was wired 2026-06-16 (Path Z Part 33)
from a blanket EOPNOTSUPP terminal to the real VFS for the two *allocate* modes:
`mode == 0` (posix_fallocate grow → `Vfs::file_size`/`Vfs::truncate`, never
shrinking) and `FALLOC_FL_KEEP_SIZE` (block reservation → `Vfs::fallocate`).
MemFd fds grow via `ipc::memfd::truncate`. Both enforce `RLIMIT_FSIZE` (EFBIG)
and the File-WRITE capability.

**Update 2026-06-18 — PUNCH_HOLE / ZERO_RANGE implemented.** The two most
commonly used range modes now do real work instead of returning EOPNOTSUPP. New
helpers `fallocate_zero_vfs` / `fallocate_zero_memfd` (kernel/src/syscall/linux.rs)
zero `[offset, offset+len)` in 16 KiB chunks via the backend's efficient
`write_at` (ext4/fat/memfs all override it). `i_size` is preserved for PUNCH_HOLE
(always KEEP_SIZE) and ZERO_RANGE+KEEP_SIZE — the zeroed region is clamped to the
current size and a range entirely past EOF is a no-op; ZERO_RANGE *without*
KEEP_SIZE grows the file to `offset+len` if the range crosses EOF, zero-filling
the gap. This is correct **read-as-zero** behaviour; the only thing not provided
vs. a real hole-punch is **disk-space reclamation** (an optimisation, not a
correctness property — our backends are non-sparse). Covered by
`self_test_fallocate_range` (registered in kernel/src/main.rs as a late, post-/tmp
self-test) which exercises ZERO_RANGE+KEEP_SIZE, PUNCH_HOLE, a past-EOF KEEP_SIZE
no-op, a ZERO_RANGE grow, and a MemFd ZERO_RANGE — all green at boot.

**Update 2026-06-20 — COLLAPSE_RANGE / INSERT_RANGE implemented.** Both
content-shifting modes now do real work for regular files (`HandleKind::File`)
instead of returning EOPNOTSUPP. The dispatch (kernel/src/syscall/linux.rs
`sys_fallocate`) enforces the full Linux contract: it queries the backing fs
block size via `Vfs::statvfs` and rejects a non-block-aligned `offset`/`len`
with EINVAL; COLLAPSE at/past EOF is EINVAL (Linux says use ftruncate); INSERT
at/past EOF (`offset >= size`) is EINVAL; INSERT also re-checks RLIMIT_FSIZE
against the *grown* size (`size + len`). The shifts themselves are chunked
(16 KiB) memmoves over `Vfs::read_at`/`write_at`: `fallocate_collapse_vfs`
slides the tail down (ascending copy, dst < src) then truncates by `len`;
`fallocate_insert_vfs` grows the file, slides the tail up (descending copy to
avoid clobber) then zeroes the inserted `[offset, offset+len)` hole. Our
backends are non-sparse, so this is a true content collapse/insert (not an
extent splice) — byte-for-byte identical from a reader's view; the only thing
not provided vs. a native ext4 extent op is the in-place efficiency, an
optimisation, not a correctness property. Covered by `self_test_fallocate_range`
cases (6)-(8): COLLAPSE_RANGE, INSERT_RANGE, and an INSERT+COLLAPSE round-trip
identity, all green at boot. A backend whose `statvfs` reports `block_size == 0`
(can't validate the alignment contract) keeps the EOPNOTSUPP fallback.

**Remaining limitation:** `UNSHARE_RANGE` still returns EOPNOTSUPP — it is a
reflink/CoW unshare concept our backends don't implement (there are no shared
extents to unshare). Well-behaved callers treat EOPNOTSUPP as "operation
unsupported" and skip it or fall back, so nothing breaks.

**Proper fix (deferred) for UNSHARE:** once a backend grows reflink/CoW extents
(none do today), dispatch UNSHARE_RANGE to a preallocate-and-unshare path; on a
non-reflink fs it is correctly a no-op (nothing is shared), so the EOPNOTSUPP
terminal is the conservative choice until reflinks exist. Kernel context
(caller_pid None) keeps the EOPNOTSUPP terminal for every mode, asserted by the
batch-536 FMODE_WRITE + vfs_fallocate gate-order self-tests.
