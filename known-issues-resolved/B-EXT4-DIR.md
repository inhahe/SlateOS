### B-EXT4-DIR. ext4 directory entries past the first block became invisible, and every directory insert grew the directory by a full block — FIXED 2026-06-16

**Symptom:** The ring-3 `link()`/`linkat()` hard-link self-test
(`self_test_linux_link`, kernel/src/proc/spawn.rs) intermittently failed
with exit 193 (link failed). Tracing showed `Vfs::write_file("/mnt/lnk-src",
b"L")` returned `Ok` but the file was then unresolvable, and later
`link()` reported `AlreadyExists` for a name the VFS layer's `exists()`
could not see. The persistent `/mnt` ext4 fixture (rootfs.ext4) also grew
without bound across boots as the self-tests created and deleted files.

**Root cause (two independent ext4 directory bugs):**

1. **`parse_dir_entries` abandoned the whole directory at the first
   `rec_len == 0`** (kernel/src/fs/ext4/driver.rs). ext4 directory data is
   a sequence of independent `block_size` chunks; a chunk can legitimately
   end with zero-padding (rec_len 0) while *later* blocks still hold live
   entries. The old loop `if hdr.rec_len == 0 { break; }` broke out of the
   entire directory, so every entry living in a block after the first
   zero-padded block was invisible to `read_dir_entries` → `dir_lookup` →
   path resolution. A file whose dirent landed in a later block "didn't
   exist" to `Vfs::exists`/`open`, yet `add_dir_entry`'s own physical scan
   still saw it (→ spurious `AlreadyExists`). It also meant `remove` could
   not find/unlink such entries, so they accumulated as orphans.

2. **`add_dir_entry`'s in-place-reuse path was dead code** (off-by-one).
   It computed the last directory block as `(dir_len / block_size) *
   block_size`, which for a block-aligned directory equals `dir_len`
   itself, so the guard `last_block_start < dir_len` was never true. Every
   insert fell through to the grow path, appending a fresh block per entry:
   unbounded directory bloat and fragmentation, which in turn fed bug (1)
   (more blocks → more chances for an entry to hide past a zero-padded
   block).

**Fix (proper):**

- Rewrote `parse_dir_entries` to parse block-by-block: an outer loop over
  `block_size` chunks and an inner loop over entries within
  `[block_start, block_end)`. `rec_len == 0` now terminates only the
  *current* block and advances to the next, never the whole directory.
  Name bounds use `block_end`, not `data.len()`. Added a regression test
  with a two-block buffer where block 0 ends in a zero-padded entry and
  block 1 holds a live entry, asserting both entries are found.
- Fixed `add_dir_entry` to compute the real last-block start as
  `dir_len.saturating_sub(block_size)` (guarded by `dir_len > 0 &&
  block_size > 0`), so free space in the final block is actually reused
  instead of growing the directory every time.
- Refactored `insert_dir_entry` to take an explicit `block_start`
  parameter (removing a buggy `(offset / remaining).max(1) * ...`
  reconstruction) and scan forward from it to find the previous entry to
  shrink.

**Verified:** With the fixes plus a freshly regenerated rootfs.ext4
(`wsl -d Ubuntu -- bash scripts/create-ext4-rootfs.sh`), the ring-3
link()/linkat() self-test passes and the full boot reaches BOOT_OK with
zero self-test failures.

**Fixture note:** The pre-existing rootfs.ext4 had accumulated duplicate /
orphaned `lnk-dst` directory entries from prior buggy boots that the fixed
code could now see but a single `remove()` could not fully clear. The
fixture was regenerated clean. `self_test_linux_link` also gained a bounded
`drain()` loop that removes any stale src/dst names before staging, so the
test is robust to a dirty persistent fixture going forward.
