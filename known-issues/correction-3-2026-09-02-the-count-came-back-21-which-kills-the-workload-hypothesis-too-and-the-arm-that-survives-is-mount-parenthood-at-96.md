## CORRECTION 3, 2026-09-02: the count came back 21, which kills the workload hypothesis too — and the arm that survives is **mount parenthood**, at 96 %

**In short:** correction 2 predicted that the next `--bench` boot would either
show `/`'s entry count tracking the regression (in which case there was never a
performance problem) or show it flat (in which case the mount walk is the place
to look). The count came back and it is **21** — nowhere near the ~70 entries of
growth the workload hypothesis needed. That is three proposed mechanisms
measured and three dead. But the same run carried a controlled experiment for
the alternative correction 2 named, and *that* one is alive: **being the parent
of mounted filesystems costs 96 % of a listing**, and it has nothing to do with
memfs, inodes, the heap, or how many files are in the directory.

### What `/` actually holds

21 entries, logged by name before anything is timed:

```
_CHANGE_CURSORS _JOURNAL _TRASH bin cwdtest etc globdir handle_dir_test
home lib lib64 reltest slateos-b2-beneath slateos-b2-decoy usr var
tmp proc dev sys mnt
```

The last five are **mount points**. That is the variable nobody was measuring:
not how many entries `/` has, but how many of them are roots of another
filesystem.

### The controlled arms

`bench_vfs_readdir_root_cost` moves one variable at a time, holding the
directory's entry count at 8 throughout, so none of these is the per-entry cost
in disguise:

| arm | what it isolates | result |
|---|---|---|
| 5 extra mounts *elsewhere* in the tree | mount-**table** size alone | 15931 → 19584 ns = **+22 %** |
| being the **parent** of those same 5 mounts | the submount walk in `finish_listing` | 19584 → 38520 ns = **+96 %**, ~3787 ns per submount |
| being a filesystem's own root | mount-root-**ness** of the listed dir | 14142 → 13159 ns = **−6 %** |

The third arm is the one that formally retires this entry's title. It is
controlled for both the mount table and the inode-table size, so
mount-root-ness is the only difference left — and it is *negative*. The
mechanism the heading names does not merely fail to dominate; it does not
exist.

### The cost model now adds up to the observed number

Correction 2 established ~10 µs fixed + ~460 ns/entry. Adding the submount term
this correction measures, `/` should cost:

```
  10.0 us  fixed
+  9.7 us  21 entries x 0.46 us
+ 18.9 us  5 submounts x 3.79 us
  -------
  38.6 us  predicted
  37.4 us  measured (vfs_readdir_root min)
```

Within 3 %. This is the first time any hypothesis in this entry has predicted
the actual number rather than merely being consistent with its sign, and that
is what makes the submount term a mechanism instead of a fourth guess.

### What has been fixed, and what has not

The by-path `stat` is gone: `finish_listing` now reads each submount root
through the mount-table handle instead of re-resolving its path. The log line
that proves it is arithmetic rather than assertion — **one** by-path stat of a
mount root still measures 5315 ns, which is *140 %* of the 3787 ns that
parenthood now costs per submount, and a fraction over 100 % is only possible
once that call is outside the loop.

What remains is `finish_listing`'s own work: the de-duplication scan and the
per-root filesystem lock, at 3787 ns per submount against the ~2000 ns an
ordinary file's `stat` costs. That is the next thing to attack, and unlike
every prior lead in this entry it is a named piece of code with a measured
price.

### Consequence for the benchmark

`vfs_readdir_root` is now recorded **unscored — workload is not fixed**, and
`vfs_readdir_32` (exactly 32 entries, no submounts) is scored in its place. The
two disagree by design: 37379 ns / 21 entries = 1780 ns per entry against
29881 ns / 32 = 934 ns. That gap *is* the submount term, which is the point —
the metric that moved under commits touching no filesystem code has been
replaced by one that cannot, and the mount cost is now measured deliberately
instead of leaking into an unrelated number.
