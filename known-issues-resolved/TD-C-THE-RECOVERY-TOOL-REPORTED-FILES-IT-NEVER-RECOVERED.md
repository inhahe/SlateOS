## TD-C-THE-RECOVERY-TOOL-REPORTED-FILES-IT-NEVER-RECOVERED -- FIXED 2026-09-15

**In short:** `apps/undelete` told people who had lost data that their files
had been recovered, with a byte count and a destination path, having written
nothing anywhere. It is the worst defect found in this tree today and the only
one that is acted upon at the moment someone is least able to check it.

**Date:** 2026-09-15. **Lane:** C.

**The flow, all of it read rather than inferred.**

1. Someone opens a file-recovery tool, which means they have lost something.
2. `UndeleteApp::new` called `simulated_partitions()` — three disks,
   `/dev/sda1` at 500 GB, `/dev/sda2` at 1 TB, `/dev/sdb1` at 2 TB — on every
   machine, whatever was attached.
3. They scan. `RecycleBinReader::scan` was, in its entirety,
   `self.entries = simulated_recycle_bin()`. The results list showed
   `/home/user/Documents/report_q4.pdf` at 245,760 bytes and
   `/home/user/Photos/vacation_001.jpg` at 3 MB.
4. They select files and press Recover. `recover_selected` built a
   `RecoveryResult` with `success: true` and
   `bytes_recovered: file.file_size` — **both taken from the invented file's
   own metadata** — for a destination nothing ever wrote to.
5. The crate contains **no reference to `std::fs` or `safeio`**. There is no
   code path in it that writes a byte to disk.

The module doc advertised "Scans ext4 filesystem inode tables and directory
entries for deleted files".

**Why this one ranks above the rest.** Every other fabrication here costs time
or trust and leaves the situation recoverable. This one closes the door: a
person told recovery *failed* keeps looking, and a person told it *succeeded*
stops — and then reformats the disk, because the data is safe elsewhere. The
false success is worse than the false failure by the exact margin that matters.

**The fix, and the half that was nearly missed.** The invented sources are gone
and recovery reports an honest failure naming the reason. That much is
straightforward. What was nearly missed is lane B's point: **"no recoverable
files" and "cannot scan" are different sentences, and an empty list is read as
the first.** An empty partition list claims you have no disks; an empty result
list claims nothing of yours survives. Both are verdicts on the user's data
that this program has not earned and cannot earn, so both screens now say
outright that nothing has looked. `an_empty_recovery_screen_says_it_could_not_look`
pins it.

**Two smaller inventions found on the way out.** The failure message read
"Data blocks partially overwritten" — a specific physical cause that was never
established, which would send someone hunting a hardware fault they do not
have. And `scan()` now does **nothing** rather than clearing: with no source to
read, it learns nothing and therefore changes nothing, where clearing would
assert the bin is empty, which is a claim it equally cannot make.

**What is implementable, and is the obvious next step.** A recycle bin is an
ordinary directory. Listing it needs `std::fs::read_dir` and restoring from it
needs a rename — neither needs the raw block-device access the inode scanner
and the signature carver do. So of the three sources this tool claims, one is
reachable today and two are not, and the honest page should eventually say that
per-source rather than as one banner.

**Twenty-four tests were resting on the invented data** — the fifth application
in a row where that was true. `test_engine_recovery` asserted that "at least
some should succeed", and it was *correct about the behaviour*: the behaviour
was the defect. A test holds a fabrication in place as firmly as it holds
anything else.
