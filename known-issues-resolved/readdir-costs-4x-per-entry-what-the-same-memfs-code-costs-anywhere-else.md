## `readdir("/")` costs 4x per entry what the same memfs code costs anywhere else

**Status:** **FIXED 2026-09-01** — see the two appended sections at the end of
this entry: MEASURED (which localised it) and FIXED (which records the change
and the after-numbers). The cause was being the parent of mount points, and
specifically the by-path stat `finish_listing` ran per submount; per-submount
cost fell 5.3x and mount parenthood fell from +237–292% of a listing to +34%.
Being a filesystem's own root was ruled out. Never a regression — it had
presumably always been true — and never a gate: the series that exposes it
(`vfs_readdir_root`) is tracked, not scored.

**In short:** listing the root directory is about four times more expensive per
entry than listing an ordinary directory, even though on the boot-test both are
the *same* in-memory filesystem running the *same* code. Something about being
the root, rather than about the entries in it, is costing roughly 29
microseconds a call. About 10 of those 29 are accounted for; the rest is not.

**Where it came from.** While refuting the two proposed causes of the
`vfs_readdir` movement (see
A-MEMFS-INODE-TABLE-MADE-VFS-READDIR-3X-SLOWER above),
`bench_vfs_readdir_breakdown` produced a cost model for a memfs listing, and the
scored root listing does not fit it. From the same boot:

| measurement | value |
|---|---|
| `vfs_readdir_n0` (empty memfs dir) | 9302 ns |
| `vfs_readdir_n8` (8 entries) | 13944 ns |
| `vfs_readdir_n64` (64 entries) | 37521 ns |
| implied slope | ~441 ns/entry |
| `vfs_readdir` — `/`, 21 entries | 47998 ns |
| implied slope for `/`, against the same fixed cost | **~1843 ns/entry** |

The model predicts `9302 + 21x441 = 18563 ns`. The measurement is 47998 ns, so
**29435 ns is unexplained by entry count**.

**This is not a filesystem-type difference.** The obvious explanation would be
that `/` is FAT and `/tmp` is memfs, and it is wrong: `main.rs:1418` mounts FAT
from `vda` and *falls back to memfs* when there is no FAT volume, and the
boot-test's `vda` is a raw swap disk with no filesystem on it. So on the machine
these numbers come from, `/` and `/tmp` are both `fs/memfs.rs`. The comparison
is like-for-like, which is what makes the gap interesting.

**What is attributed (about a third).** `Vfs::finish_listing`
(`kernel/src/fs/vfs.rs:2333`) injects mount points that the backing filesystem
does not know about, and for each one it calls `submount_root_ino`
(`vfs.rs:3454`), which is:

```rust
Self::metadata_resolved(dir_path.join(name)).map_or(0, |m| m.ino)
```

That is a **full stat per mount point** — a `PathBuf` allocation for the join,
then `resolve_mount` (VFS lock, longest-prefix scan) and a filesystem
`metadata()` call. `/` has five submounts (`tmp`, `proc`, `dev`, `sys`, `mnt`),
and the suite already prices exactly this operation: `vfs_stat_breakdown_resolved`
= 1928 ns. Five of those is **9640 ns**, or 33% of the gap. It also explains the
shape — the cost attaches to the *directory* (how many things are mounted under
it), not to how many entries it holds, which is precisely the anomaly.

The `!entries.iter().any(...)` de-duplication in the same loop is **not** a
suspect: 5 mounts x 21 entries is ~105 `PathBuf` comparisons, nowhere near
microseconds.

**What is not attributed.** ~20 us. Candidates, in the order worth testing:
`resolve_follow` + `check_path_access` on the root path itself; `submount_children`
taking the `VFS` lock a second time (`finish_listing` takes it, then each
`submount_root_ino` takes it again through `resolve_mount`); and memfs's own
root-directory lookup, which may not be structured like a child directory's.

**The proper fix (once it is measured, not before).** `finish_listing` already
holds the mount table when it computes `submount_children`; the mounted
filesystem's root inode is available from that same table without re-resolving a
path that was just constructed from data the table already had. Reading it there
would remove five stats, five joins and five lock acquisitions per root listing.
But that is the fix for the attributed third — the remaining two thirds must be
measured first, because this whole entry exists downstream of an investigation
that lost two days to acting on an unmeasured hypothesis.

**How to measure it.** The same way the last one was settled: a 2x2 in
`bench_vfs_readdir_breakdown` — the same directory listed as a mount parent and
as a plain child — rather than reasoning about the code. `bench_vfs_readdir`
now logs the entry count of `/` and names every boot, so the workload side of the
comparison is finally pinned.

### MEASURED 2026-09-01 — it is the mount parenthood, and it is 8x the estimate

`bench_vfs_readdir_root_cost` (`kernel/src/bench.rs`) ran the 2x2. It lists one
8-entry memfs directory four times, varying only what is mounted and where:

| arm | mount table | listed dir is their parent? | run 1 | run 2 |
|---|---|---|---|---|
| `vfs_readdir_mp_base` | baseline | — | 20990 ns | 23747 ns |
| `vfs_readdir_mp_elsewhere` | +5, under a *sibling* | no | 27807 ns | 28998 ns |
| `vfs_readdir_mp_parent` | +5, under the listed dir | **yes** | **109236 ns** | **97932 ns** |
| `vfs_readdir_mp_submount_stat` | one stat of a mount root | (not a listing) | — | 8740 ns |
| `vfs_readdir_fsroot` | +1, the listed dir *is* the mount | — | 22626 ns | 21529 ns |

**Read the percentages, not the nanoseconds.** The harness flagged *both* boots
as outlier runs — everything ~38% and ~22% slower than usual respectively — and
printed the standing instruction not to quote their absolute numbers as the cost
of anything. The design survives that: within each run all arms shared one
window, so a uniform inflation cancels in every comparison below. The ratios are
the finding; the raw figures are high by roughly those factors. Two runs are
reported because the second was run to add the `submount_stat` arm, and having
them replicates the headline independently.

Four results, in decreasing order of how much they change the picture:

1. **Being a mount parent costs +292% (run 1) / +237% (run 2)** — measured
   against `elsewhere`, so mount-table growth is already subtracted out. That is
   +81429 ns and +68934 ns, i.e. the entire 29.4 us gap and ~2.5x more, on a
   directory with a *quarter* of root's entries. Root's anomaly is here, in
   `finish_listing`'s per-submount work, and nowhere else. Replicated.

2. **It is mostly the stat — but the stat costs 4x what the estimate assumed.**
   One `metadata_resolved` of a *mount root* measures **8740 ns**, against
   ~2000 ns for `vfs_stat_breakdown_resolved` on an ordinary file. Five of those
   is 43700 ns, which is **63% of run 2's 68934 ns**. So the estimate above
   ("five full stats = 9640 ns, 33% of the gap") had the mechanism right and was
   low only because it priced a mount-root stat at an ordinary file's cost. The
   proposed `finish_listing` fix is the right fix and should recover most of
   this.

   *(An earlier revision of this section said the opposite — "it is not the stat
   … the fix will not recover most of this". That was written from run 1, which
   had no `submount_stat` arm, and it was an inference, not a measurement. Run 2
   added the arm and contradicted it. Left visible rather than quietly
   overwritten, because the same mistake is what this whole entry is downstream
   of.)*

3. **The remaining ~37% is `finish_listing`'s own loop** — the `PathBuf` join per
   mount point and the `!entries.iter().any(...)` de-dup scan. ~5000 ns per
   submount. Worth a second look after the fix lands, not before: the fix changes
   that loop, so measuring its remainder now would price code about to be
   replaced.

4. **Being a filesystem's own root is free.** `fsroot` came out +1636 ns (+7%)
   over `base` in run 1 and **−2218 ns (−9%)** in run 2 — bracketing zero, which
   is stronger evidence than either run alone. That eliminates the third
   candidate listed above ("memfs's own root-directory lookup, which may not be
   structured like a child directory's"). It is, and it costs the same.

A fifth number falls out that was not the question but is worth recording:
**each registered mount adds ~1000–1400 ns to every `readdir` in the system**,
whatever it lists (`base` -> `elsewhere` is +6817 / +5251 ns for five mounts, on
a directory none of them are under). That is `resolve_mount`'s longest-prefix
scan, and it is the reason this benchmark mounts its five filesystems twice in
two different places rather than measuring a naive before/after — charged to
`finish_listing`, it would have inflated result 1 by 8%.

**Next step: apply the fix.** `finish_listing` already holds the mount table when
it computes `submount_children`; the mounted filesystem's handle is right there
in the table entry. Cloning the `Arc` alongside the name, releasing the VFS lock,
then reading each root's metadata through that handle removes the join and the
whole `resolve_mount` longest-prefix scan per submount, while preserving the
§43 lock discipline that `submount_root_ino`'s doc comment turns on (VFS lock
released before any filesystem lock is taken). The 2x2 above is the before/after
harness for it.

### FIXED 2026-09-01 — commit `7e3b02158`, per-submount cost down 5.3x

`submount_children` now clones the mounted filesystem's `Arc` out of the mount
table alongside the name, and `submount_root_ino` stats that handle's root
directly instead of rebuilding a path and re-running `resolve_mount`. `Vfs::mount`
refuses a duplicate mount path (`vfs.rs:1681`), so each path maps to exactly one
table entry and the two are equivalent — that is what makes the substitution
safe rather than merely faster. §43 lock discipline is unchanged: the VFS lock is
still released before any filesystem lock is taken, which the old code needed
too (it took the same filesystem lock inside `metadata_resolved`).

The 2x2 as before/after, all figures the controlled `parent` − `elsewhere`
comparison so mount-table growth is subtracted out:

| | base | elsewhere | parent | delta | per submount | parenthood costs |
|---|---|---|---|---|---|---|
| before, run 1 | 20990 | 27807 | 109236 | 81429 | 16286 ns | **+292%** |
| before, run 2 | 23747 | 28998 | 97932 | 68934 | 13787 ns | **+237%** |
| after, clean run | 29986 | 37551 | 50530 | 12979 | **2595 ns** | **+34%** |

**Per-submount cost fell from ~13800–16300 ns to 2595 ns — 5.3x, or 81% of it
removed.** The scored `vfs_readdir_root` fell from 90145 ns on the run
immediately before the fix to 36958 ns and then 37872 ns on the two runs after
it (the harness scored the first of those `-50% vs suite, -59% raw`), so the
headline is replicated across two independent boots.

**Why there are three after-runs and only one is quoted.** The first post-fix
boot was a `MEASUREMENT VOID`: `mp_parent` and `mp_elsewhere` came back 92% and
82% split-half apart, and `elsewhere` landed *below* `base` — impossible, since
five more mounts can only make `resolve_mount`'s scan longer. Its numbers are
not in the table. The quoted run has all five arms at 0–1% split and none of
them in the dispersion list; the run is flagged contaminated on the canary
instrument alone, with dispersion and wall time both inside the host's band.

**The benchmark's own verdict was wrong on that void run, and that is fixed
too** (commit `3d140217f`). It printed a confident `STATS-DOMINATE` from samples
the host-side report had just discarded, told the reader "the fix should recover
most of this" about a binary that already contained the fix, and reported an
impossible negative mount-table delta as a finding. Three guards were added: the
ladder now refuses to conclude when any arm it rests on is split-half unstable;
a negative `d_table` is named as noise instead of narrated, and is withheld from
the fs-root arm's correction; and a `STATS-REMOVED` branch keys on `explained`
exceeding 100%, which is only possible once the by-path stat is out of the loop.
`explained` is deliberately not clamped — clamping is what made the void run
misreport the fix as unapplied. See design-decisions.md §670.

**What is left, and it is small.** 2595 ns per submount remains, against ~2000 ns
for an ordinary file's stat. That is now `finish_listing`'s own loop — the
`!entries.iter().any(...)` de-dup scan and the per-root filesystem lock — plus
the mount-root stat itself, which is irreducible (the entry has to report an
inode). Not worth chasing: the remaining per-submount cost is within ~30% of a
plain stat, so there is at most ~600 ns per submount of overhead left to find.

**One caveat on arm 4 that is now visible and was not before.** `vfs_readdir_fsroot`
came out −62% below `base` this run (−54% the run before), which reads as "being
a filesystem's own root is much *faster*". It is not a finding: `base` lists a
directory inside the **root** filesystem, which holds the whole boot image's
inodes, while `fsroot` lists a freshly-mounted memfs holding eight. The
`vfs_readdir_breakdown` arms in the same boot measure that confound directly —
2048 extra inodes elsewhere move an 8-entry listing by 7567 ns (53%). So arm 4
is not controlled for inode-table size and its magnitude is uninterpretable. The
*direction* is all it supports, and the original conclusion stands on that: being
a filesystem's own root is not expensive. Logged as tech debt below.
