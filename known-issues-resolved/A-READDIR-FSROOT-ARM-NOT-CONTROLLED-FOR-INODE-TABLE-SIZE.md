## A-READDIR-FSROOT-ARM-NOT-CONTROLLED-FOR-INODE-TABLE-SIZE

**Status:** FIXED 2026-09-01 in `91b0f753e` — arm 4b landed and the report line
now compares against it instead of against `base`. The first controlled reading
came off a **contaminated** boot, so the number itself is not yet quotable; see
"What the controlled arm measured so far" at the bottom. The debt — a benchmark
making an unfair comparison and printing the result as a finding — is closed.

*The rest of this entry is the original text, kept because it is the argument
for the fix.* Tech debt in a benchmark, not a kernel bug. The arm's *direction*
was sound and its conclusion stood; only the magnitude it printed was
uninterpretable.

**In short:** one arm of a benchmark compares two directories that differ in two
ways at once, and reports the difference as if they differed in only one. It
prints a number like "−62%" that reads as a discovery and is mostly an artifact
of the comparison being unfair. Nothing in the kernel is wrong; the benchmark
just cannot support the size of the claim it makes.

**Where:** `kernel/src/bench.rs`, `bench_vfs_readdir_root_cost`, arm 4
(`vfs_readdir_fsroot`), and the report line that compares it against
`vfs_readdir_mp_base`.

**What it does.** Arm 4 asks whether listing a directory that *is* a mounted
filesystem's root costs more than listing an ordinary child directory holding
the same eight entries. It mounts a fresh `MemFs`, puts eight files in it, and
lists it — comparing against `base`, which lists a plain directory holding eight
files.

**Why the comparison is not controlled.** `base`'s directory lives in the **root**
filesystem, whose inode table holds the entire boot image. Arm 4's memfs is
brand new and holds eight inodes. `bench_vfs_readdir_breakdown`, in the same
boot, measures exactly this confound: 2048 extra inodes elsewhere in the same
filesystem move an 8-entry listing by 7567 ns (53%), and the whole reason that
benchmark exists is `A-MEMFS-INODE-TABLE-MADE-VFS-READDIR-3X-SLOWER`. So arm 4
varies *two* things — mount-root-ness and inode-table size — and attributes the
sum to the first.

**How it shows.** The arm reports `fsroot` as *faster* than `base`, by −54% and
−62% on the two 2026-09-01 runs after the `finish_listing` fix. A directory
cannot be cheaper to list for being a mount root; the negative is the
inode-table difference showing through with the opposite sign.

**Why it was not visible earlier.** The first two runs put it at +7% and −9% —
bracketing zero, which looked like a clean "no effect" result and was read as
one. Those runs were heavily contaminated (~38% and ~22% slow), which
compressed the arm toward the rest of the suite. The cleaner the run, the more
plainly the confound shows.

**What the arm still supports.** Its direction, which is all the parent entry
ever claimed: being a filesystem's own root is *not* expensive. Since the
uncontrolled difference biases the arm toward looking fast, a null-or-negative
result rules out a *positive* cost a fortiori. That is why
`readdir("/") costs 4x per entry` was closed with this arm's conclusion intact.

**The proper fix.** Give arm 4 a fair partner: mount a second fresh `MemFs`
elsewhere, populate it with the same eight files, and list a *child* directory
inside it. Then both sides of the comparison sit in an equally-sized inode
table, and the only remaining difference is mount-root-ness. Cheap to do — it
is one more mount and one more populate call in a function that already has
helpers for both (`readdir_mount_populate`, `readdir_mount_cleanup`) — and it
would turn a direction into a measurement.

**Until then**, the report line should be read as "not positive", never as a
speedup, and the printed percentage should not be quoted anywhere. The FIXED
section of `readdir("/") costs 4x per entry` states that caveat inline for the
same reason.

---

### FIXED 2026-09-01 — commit `91b0f753e`, arm 4b

**What landed.** `vfs_readdir_fsroot_peer`: a second fresh `MemFs` mounted at
`/tmp/_rdm/p`, with the eight files one level down in `/tmp/_rdm/p/c`, so the
listed directory is an ordinary *child* of a filesystem whose inode table is the
same size (ten inodes against nine — the extra is the child directory itself).

Two things were controlled, not one. The inode-table confound is the one this
entry was filed about. The second is the mount table: **`peer` is mounted before
either measurement window**, so both arms see the same number of mounts. That is
what removed the `table_1` correction the old comparison had to apply — the
correction existed only because the old arms were measured across *different*
mount tables, and it was the smaller of the two errors anyway. Mount-root-ness
is now the only difference between the two sides.

The report block matches on `(fsroot_result, peer_result)` and has three arms:
controlled when both are present (carrying the same split-half VOID guard §670
put on the rest of the ladder), explicitly "**NOT quotable** as a mount-root
cost" when the peer could not be set up — rather than silently falling back to
the biased comparison, which is the failure this entry describes — and
UNAVAILABLE when the fs-root mount itself failed.

**What the controlled arm measured so far.** One boot, and it is not a reading:

| | uncontrolled (old) | controlled (new) |
|---|---|---|
| comparison | `fsroot` vs `base` | `fsroot` vs `peer` |
| result | −54%, −62% | −16% (19697ns vs 23637ns) |

The confound accounted for roughly three quarters of the apparent effect, which
is the right order — `bench_vfs_readdir_breakdown` prices 2048 extra inodes at
53%, and `base`'s filesystem holds the whole boot image.

**Why −16% is not yet a finding.** That boot was `RUN CONTAMINATED` (reference
access cost spread 115% over 15 samples against a 25% tolerance), and the
positional attribution puts both new arms in the worst of it: `vfs_readdir_fsroot`
ran at 1.50x the run's baseline reference cost and `vfs_readdir_fsroot_peer` at
1.59x — positions 55 and 56, inside the same disturbance. Their split-halves
(12% and 15%) passed the kernel-side guard, so the line printed rather than
voiding, but a host that moved 115% under the canary is not a host to read a
16% difference from.

**A negative here is not impossible, unlike `d_table`.** Worth stating because
the ladder treats a negative mount-table delta as proof of noise, and the two
are different. For a mount root, `find_mount` yields the relative path `/` and
the per-filesystem lookup is the root directory directly; the peer's child needs
one more component walked inside its memfs. The fs root genuinely does less
work, so "cheaper" has a mechanism and the sign is not self-refuting. What is
missing is a quiet boot, not an explanation.

**Trigger for closing the number out:** the next `--bench` boot that is not
contaminated. Nothing depends on it — the parent entry's conclusion never rested
on the magnitude, only on the direction, and the direction is unchanged.
