## A-MEMFS-CANNOT-HARD-LINK-SO-NO-BOOT-EVER-TESTS-A-HARD-LINK — FIXED 2026-09-01 (lane A, filed 2026-08-31)

**Status: FIXED.** memfs now keeps a flat inode table (`BTreeMap<ino, node>`)
with directories holding `name -> ino`, and implements `link` /
`link_no_follow`. See `design-decisions.md` §665, which also reverses §30's
June decision to defer exactly this refactor.

What that changed, in the terms this entry was filed in:

- The recorded skip (`pinned linkat: the positive same-inode/nlinks
  assertion`) is **deleted**, not merely unreached — the assertion is stated
  unconditionally, so a regression fails the boot rather than restoring the
  skip.
- `nlink` and `st_ino` now mean on memfs what the entry below notes they were
  only half-promising: `nlink` counts the names, and `drop_link` frees the
  object when the last one goes.
- `self_test_linux_link` (ring 3) falls back from `/mnt` to `/tmp` when there
  is no ext4 mount. Insurance, not a repair — see the correction below.
- `memfs::self_test` gained direct coverage: shared inode, shared data, unlink
  decrementing rather than deleting, follow vs no-follow, and no inode leak.

**Correction to this entry's own title and premise, 2026-09-01.** The title
says "NO BOOT EVER TESTS A HARD LINK", and the report below says "no boot has
ever exercised a hard link". Both are **false**, and were false when filed.
Checked against the serial log and `scripts/check-boot-skips.py --list`:

- `scripts/boot-test.sh` attaches `rootfs.ext4` as vdb; `main.rs` probes vdb/vdc
  and mounts the first ext4 it finds at `/mnt`. So `/mnt` exists on every boot
  the harness runs.
- `self_test_linux_link` therefore ran, on ext4, in **all 14** boots in the skip
  window — it records no skip. The serial log for this very boot reads
  `Running Linux link()/linkat() hard-link test (ring 3, ext4 /mnt)... OK`.

What *was* true is narrower and still worth the fix: the assertions that need
the **root** filesystem to have hard links could not run. `pinned linkat`'s
positive same-inode/`nlinks` half skipped in **7 of those 14 boots** because it
works in `/tmp`, and memfs had no hard-link coverage of its own because it had
no hard links. That is the gap this refactor closed; "no boot ever tests a hard
link" overstated it, and the overstatement was in the direction that made the
work look more necessary than it was. The title is left as filed so the ID
stays stable and searchable.

The original report follows unedited (including the false sentence above).

---

**What.** `kernel/src/fs/memfs.rs` implements no `link`/`link_no_follow`, so
`FileSystem::link`'s default applies and every hard link on a memfs mount
returns `NotSupported`. `/tmp` is memfs, and `/tmp` is where the VFS self-tests
run — so **no boot has ever exercised a hard link**, on either the path route
(`SYS_FS_LINK`) or the new pinned one (`SYS_FS_LINKAT_PINNED`, §658).

**How it was found.** The new pinned `*at` self-test asserted that
`link_at_pinned` produces a second name for the *same inode*. It failed the
boot with `NotSupported` — from memfs, not from the primitive. Until that
assertion was written, nothing had ever asked memfs to make a link.

**Why memfs cannot.** Its tree is nodes owned **by value**: a `Dir` holds
`children: BTreeMap<String, MemFsNode>`. Two names sharing one node is
therefore not expressible. Real hard links need an inode table — directory
entries mapping name → ino, with the node bodies held separately and
refcounted — which is a restructure of the module rather than an added method.
Note memfs already allocates a unique synthetic `st_ino` per node
(`alloc_memfs_ino`), precisely so `cp -a`/`tar` hard-link dedup can work; that
promise is only half-kept, since `nlink` can never exceed 1.

**Current state.** The pinned self-test treats `NotSupported` from
`link_at_pinned` as a recorded skip (`pinned linkat: the positive
same-inode/nlinks assertion`) and continues. This is safe but must not become
permanent: everything that makes the primitive *safe* — `check_at_name` and
both `verify_pinned` passes — runs before the filesystem is reached, so the
containment and stale-handle assertions do run. What is unproven is that a
link, when it succeeds, actually shares an inode and raises `nlink`.

**Proper fix.** Give memfs an inode table and implement `link`/`link_no_follow`
against it, then delete the skip. That also gives `SYS_FS_LINK` its first
self-test coverage, and it makes `nlink` and `st_ino` mean on memfs what they
already claim to mean.

**Watch it.** `scripts/check-boot-skips.py` will start reporting this skip on
100% of boots once ten qualifying boots are recorded — which is the mechanism
for making sure the skip above does not quietly become the permanent answer.
