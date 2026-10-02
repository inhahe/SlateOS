## §245 — The ZFS driver reads the pool's own attribute registry instead of hardcoding the offsets every ZFS bootloader hardcodes

**Date:** 2026-08-20
**Decided by:** Claude (autonomous)
**Lane:** A

**In short.** SlateOS can now read a ZFS disk. In ZFS, a file's basic facts —
its size, its permissions, when it was last modified — are not at fixed
positions in the file's on-disk record. Each file says "I use layout 7", and the
disk carries a table saying what layout 7 contains and in what order. Every
other ZFS reader that only needs to boot (FreeBSD's loader, GRUB) skips that
table and assumes the common layout, because it almost always is. The decision
here was to read the table instead. The cost is roughly a third more code and
two extra features that only exist to support it; the benefit is that a file
with an access-control list, an extended attribute or a project id is read
correctly rather than being reported with a wrong size and wrong permissions and
no error.

**Decision.** `fs::zfs::sa` walks the filesystem's `SA_ATTRS` object to its
`REGISTRY` and `LAYOUTS` ZAPs and resolves every attribute's offset from them at
mount time. Nothing in the driver contains an offset like `SA_MODE_OFFSET 0`.

**Why the hardcoded route is tempting.** It is what the reference readers do,
and it is a dozen lines against several hundred. ZFS lays out the common case
identically on every pool, so a hardcoded reader is correct for the overwhelming
majority of files on the overwhelming majority of pools — including, almost
certainly, every pool this driver would ever be pointed at in testing.

**Why it was rejected.** The failure is silent and it is a *wrong answer*, not
an error. A file with an ACL has a different layout number; the attributes after
the changed one shift, and a hardcoded reader returns whatever bytes now sit at
offset 8 as the file's size. Nothing about that looks like a failure at any
layer: the mount succeeded, the directory listed, the `stat` returned. It would
surface as a truncated copy, which is the worst possible way for a filesystem
driver to be wrong. A read-only driver's entire value is that what it hands back
is what is on the disk.

**What it cost.** Two features exist solely because of this choice:

| Feature | Why the registry needs it |
|---|---|
| ZAP array-valued lookup (`zap::lookup_array`) | A layout is a big-endian `u16` array, not the single `u64` every other ZAP read in the driver wants |
| dnode spill-block parsing (`dmu::Dnode::spill`) | A file whose attribute set does not fit the bonus buffer keeps it in a spill block, which is found from the *end* of the dnode's slots rather than after its block pointers |

Both are real format features that a complete driver needs anyway; the registry
is what forced them to be written now rather than later.

**The corollary decision: an unreadable registry is a refusal, not a fallback.**
If the SA registry cannot be read on a ZPL v5 filesystem, `open_source` returns
`CorruptedData` rather than falling back to the assumed layout. A fallback would
reintroduce exactly the silent-wrong-answer this entry exists to avoid, on
precisely the pools where something is already unusual.

**Bonus and spill are two independent buffers, not one continuous set.** Each
carries its own `sa_hdr_phys_t` and its own layout number, matching OpenZFS
`sa_build_index`. The obvious reading — "the attributes continue in the spill" —
concatenates them and resolves one layout across the join, which puts every
spilled attribute at the wrong offset. This is the same class of bug as the
hardcoded offsets, reached by a different route.

**What would change this decision.** Nothing about correctness. If the mount
path ever becomes hot enough that two extra ZAP walks matter, the fix is to
cache the resolved registry per-pool — which the driver already does, once per
mount — not to stop reading it.
