## 30. memfs hard links — leave unsupported (spec-correct EPERM); test `link(2)` on ext4 instead of refactoring memfs to an inode table

> **SUPERSEDED 2026-09-01 by §665.** Option (A) below — the inode-table
> refactor — was done. What this entry got wrong is not that ext4 went untested
> (it did not: the boot test attaches `rootfs.ext4` as vdb, `/mnt` is mounted,
> and the ring-3 `link(2)` rung has run on ext4 in all 14 boots in the skip
> window). It is that "test it on ext4 *instead*" quietly conceded every
> assertion that can only be made where the filesystem under test is the root —
> above all `[vfs] pinned linkat`, which works in `/tmp` and therefore **skipped
> its positive same-inode/`nlinks` assertion in 7 of those same 14 boots**. One
> filesystem passing is not the property; *every* filesystem claiming `link`
> support honouring it is. The entry is kept unedited below, because the
> reasoning it records is the reasoning §665 has to answer.

**Date:** 2026-06-16

**Decided by:** Claude (autonomous) — reversible; the operator may overrule. Made
while wiring the `link`/`linkat` syscalls (Path Z Part 28) and discovering the
boot root FS (memfs) cannot represent a hard link.

**Context:**
The Linux-ABI `link`/`linkat` syscalls were stale `EROFS` stubs. Wiring them to
`Vfs::link` (the proper syscall-layer fix) exposed that the in-memory root FS
(`/`, `/tmp`) cannot create hard links: memfs is a tree of by-value `MemFsNode`s
where each directory's `BTreeMap<String, MemFsNode>` *owns* its children and a
regular file stores its bytes inline (`MemFsNodeKind::File(Vec<u8>)`). Two
directory entries therefore cannot reference one shared inode — exactly what a
hard link requires (shared data, shared metadata, shared `nlink`). The default
`Filesystem::link` returns `NotSupported`; ext4 (`fs/ext4/vfs_impl.rs`)
implements real hard links.

**Options considered:**
- **(A) Refactor memfs to an inode-table model** — `MemFs` owns
  `BTreeMap<ino, Inode>` (data + metadata + `nlink`), and file/symlink directory
  entries hold an `ino` instead of the body, so multiple names can share one
  inode; `remove` decrements `nlink` and frees on zero. This is the textbook
  design and would make hard links work on the *actual* running root. But it is
  a sweeping rewrite of a core subsystem touching every memfs operation (read,
  write, truncate, metadata, lstat, remove, rename, the directory walk) and the
  many memfs self-tests, with no current consumer demanding it.
- **(A′) Share only file *bodies* via `Rc`/`Arc`** — keep the tree, wrap file
  data (and metadata) in a refcounted cell. Rejected: `Rc` is not `Send`/`Sync`,
  which would poison the global `Mutex<Vfs>` static; `Arc<Mutex<…>>` nests locks
  inside the already-held VFS lock. More complexity than the clean inode table
  for no extra benefit.
- **(B, chosen) Leave memfs returning "unsupported" and test on ext4.** memfs
  reporting no hard-link support is **spec-correct**: Linux `link(2)` returns
  `EPERM` for filesystems that don't support hard links. The `link`/`linkat`
  syscall wiring is complete and correct; it returns the FS's real answer
  (works on ext4, declines on memfs). The Part 28 regression test exercises the
  success path on the ext4 mount at `/mnt`.

**Reasoning:** The roadmap item is *syscall* fidelity (stop being a blanket
`EROFS` stub), which option B fully achieves. ext4 is the design's real root FS
(`ext4 first`), and it supports hard links — so the practically important case
already works. memfs is the diskless/early/`/tmp` fallback; hard links there are
not a real workload requirement today. Doing the large, risky inode-table
refactor speculatively would violate "don't restructure a core subsystem without
a concrete need," and the deferral is cleanly reversible.

**Known limitation (tracked):** hard links are unsupported on memfs-backed paths
(`/`, `/tmp`) — see known-issues.md B-SYM1. The proper fix (inode-table memfs) is
recorded there and here for when a consumer needs it.

**How to reverse:** implement `Filesystem::link` for memfs via the option-A
inode-table refactor; the syscall wiring needs no change (it already delegates to
`Vfs::link`). The Part 28 test could then also run against `/tmp`.
