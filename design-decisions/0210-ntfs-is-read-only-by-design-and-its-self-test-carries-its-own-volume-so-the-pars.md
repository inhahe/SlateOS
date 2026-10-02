## §210 — NTFS is read-only by design, and its self-test carries its own volume so the parser is exercised on every boot

**Date:** 2026-08-16
**Decided by:** Claude (autonomous)
**Lane:** A
**Where:** `kernel/src/fs/ntfs/` (`source.rs`, `raw.rs`, `boot.rs`,
`record.rs`, `attr.rs`, `index.rs`, `mod.rs`, `tests.rs`),
`kernel/src/fs/mod.rs`, `kernel/src/main.rs` (self-test call site)

**In short:** NTFS is the filesystem on every Windows disk, so a dual-boot
machine's other partition is unreadable to us without a driver for it. This
adds one that can *read* an NTFS volume but refuses every write, and that
tests itself against a fake NTFS disk it builds in memory rather than needing
a real one plugged in. Three calls were made along the way, each with a
genuine alternative: read-only rather than read-write; a synthetic in-RAM
volume rather than a device-gated test; and, for a filesystem whose names are
case-insensitive on an OS whose names are case-sensitive, what
`/HELLO.TXT` should open when the file on disk is `hello.txt`.

### Decision 1 — read-only, and the mount says so

NTFS keeps itself consistent through `$LogFile`, a write-ahead journal
(a log of intended changes written before the changes themselves, so an
interrupted write can be undone). Every Windows NTFS write goes through it.
A driver that writes *without* it can leave a volume in a state `chkdsk`
cannot repair — not "some lost data", but a directory tree Windows will
refuse to mount. Implementing `$LogFile` correctly is a far larger job than
the read path, and getting it subtly wrong is worse than not writing at all,
because the damage is silent until the user next boots Windows.

So `statvfs` reports `read_only: true` and every write path returns
`ReadOnlyFilesystem`.

*The consequence that needed a decision*: what `metadata()` should report for
permissions. Inventing `0o644` — what almost every driver does — is a lie:
userspace checks the mode, sees a writable file, opens it for writing and
fails at the syscall. The driver reports `0o444` for files and `0o555` for
directories, so the mode agrees with what the mount will actually do. The
per-file DOS read-only bit deliberately does *not* narrow this further:
while the whole mount refuses writes, reporting a narrower mode for the
flagged files would imply the unflagged ones are writable.

*Alternative rejected*: read-write with `$LogFile`. Not "too much work" — the
issue is that a half-correct journal is indistinguishable from a correct one
until the failure that tests it, and that failure destroys a user's Windows
install. Revisit only with a full `$LogFile` implementation and a crash-
injection test, not as an increment on this code.

### Decision 2 — the self-test builds its own volume (`SectorSource`)

The precedent to avoid is `fs::iso9660::self_test()`. It runs five pure unit
tests and then prints *"No ISO 9660 filesystem mounted — skipping integration
test"* — which, in the CI boot test, is every time. Its hard parts (volume
descriptors, directory records, extents) have therefore never been executed by
a test. NTFS is worse-shaped for that: **all** its hard parts are on-disk
structures. A device-gated test would leave fixups, runlists, the `$I30`
B+ tree and `$ATTRIBUTE_LIST` entirely uncovered.

So the driver reads through a `SectorSource` trait with two implementations:
`DeviceSource` (the block cache) and `MemorySource` (a `Vec<u8>`). The
self-test hands it a synthetic volume built byte-by-byte by `tests.rs` and
drives the *entire* parser — mount, directory listing, path resolution, file
reads — with no device attached.

Two properties of that builder were deliberate:

- **It is written independently of the parser, not from shared code.** A
  builder that reused the parser's notion of the layout would agree with it by
  construction, including where both are wrong. `tests.rs` writes the fields
  from the on-disk format documentation; `write_fixups` is a separate
  implementation of the write side of `apply_fixups`.
- **It covers each structural fork exactly once**, because NTFS's forks are
  the kind where code that handles one branch passes on whichever volume it
  was written against: resident vs. non-resident `$DATA` (a 100-byte file has
  no clusters at all); an INDX-backed directory vs. a fully resident one (a
  small directory has no `$INDEX_ALLOCATION`, a large one's `$INDEX_ROOT`
  holds nothing but a child pointer — so a driver reading only the root lists
  small directories perfectly and large ones as empty); a fragmented runlist;
  a sparse run; `$ATTRIBUTE_LIST` with the `$DATA` in an extension record; an
  8.3 DOS alias that must not be listed; a *negative* `clusters_per_mft_record`
  (the signed power-of-two encoding a naive parser reads as 246); and a
  non-zero MFT sequence number, so an unmasked 64-bit file reference shows up
  as an absurd record number instead of silently working.

*Alternative rejected*: shipping a real `.ntfs` image as a test fixture. It
would be more authentic, but it is an opaque binary in git that nobody can
review, it cannot be minimised to the cases above, and adding a new case means
regenerating it on a Windows machine. The builder is readable, diffable, and
each structure it emits sits next to a comment saying why that case exists.

*Alternative rejected*: a `#[cfg(test)]` host-side test. The kernel's test
story is the boot self-test; a host test would not run in CI at all.

### Decision 3 — exact match first, then a *unique* case-insensitive match

This OS is case-sensitive by design (`design.txt`); NTFS's `$I30` index
collates case-**in**sensitively, which means a Windows-created volume cannot
contain two names differing only in case. A path lookup therefore tries the
exact name first, and only if that misses does it fold case — and if the fold
matches **more than one** entry, the lookup fails with `NotFound` rather than
picking one.

The reasoning: `/HELLO.TXT` on a volume containing only `hello.txt` has
exactly one plausible meaning, and refusing it makes a Windows disk unusable
from a Windows-shaped path. But the collation guarantee is Windows's, not
ours — a volume built by another tool (or a `POSIX`-namespace name, which NTFS
does allow to be case-sensitive) can contain both. Where it does, opening
*the wrong file* is a worse outcome than opening none, so ambiguity is
refused rather than resolved.

*Alternative rejected*: pure case-sensitivity. Consistent with the OS rule,
but it makes half the paths a user types at a Windows disk fail for no reason
the user can see.

*Alternative rejected*: pure case-insensitivity (fold always, first match
wins). Simple, matches Windows — but it silently picks between two real files
when both exist, and it contradicts the OS-wide rule at the one place a user
would notice.

### Related refusals, for the same reason

Three other places return an error rather than a plausible-looking answer, on
the principle that a filesystem that guesses is worse than one that stops:

| Situation | Response | Why not the alternative |
|---|---|---|
| Compressed or encrypted `$DATA` | `NotSupported` | Handing back the raw compressed bytes as file contents is data corruption that looks like success. |
| A sector whose fixup does not match | `CorruptedData` | This is the *only* torn-write detector NTFS has. Downgrading it to a warning does not make the driver permissive, it makes it return half-old data. |
| An `INDX` block whose VCN is not the one requested | `CorruptedData` | Fixups prove the block was written atomically, not that it is the block we asked for. A runlist bug yields a valid block from elsewhere *in the same directory* — real names in the wrong place, which passes every other check. |

**How to reverse:** read-only is the load-bearing one; undoing it means
implementing `$LogFile` and is a new project, not an edit. The `SectorSource`
indirection costs one `dyn` call per sector read on a path that is already
doing I/O, and could be removed by monomorphising `NtfsFs<S: SectorSource>` if
that ever measures. The lookup rule is one function — `ntfs::lookup` in
`mod.rs` — and switching to either pure policy is a few lines there.
