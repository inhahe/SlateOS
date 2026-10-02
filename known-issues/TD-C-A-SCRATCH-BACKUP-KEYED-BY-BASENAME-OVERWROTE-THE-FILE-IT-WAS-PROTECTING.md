## TD-C-A-SCRATCH-BACKUP-KEYED-BY-BASENAME-OVERWROTE-THE-FILE-IT-WAS-PROTECTING

**In short:** a throwaway verification script backed up two files before
breaking them on purpose, then restored them afterwards. Both files were named
`main.rs`, the backup was keyed by that name alone, so the second file's copy
silently replaced the first's -- and the restore wrote one app's entire source
over the other's. No error was raised at any point.

**Date:** 2026-09-15. **Lane:** C. Not a defect in the OS; a hazard in the
scripts written to work on it, recorded because the layout that causes it is
this project's universal one.

```python
for f in ["apps/sysinfo/src/main.rs", "apps/benchmark/src/main.rs"]:
    shutil.copy(f, backup / pathlib.Path(f).name)   # both are "main.rs"
```

Every application here is `apps/<name>/src/main.rs`. **Any sweep script that
touches two apps and keys anything by file *name* aliases them**, and
`shutil.copy` reports success on the collision. The failure is silent at write
time and destructive at restore time, so the damage surfaces long after the
line that caused it -- here, as a `grep` for a function that had been added
minutes earlier returning nothing, in a file that had become another app.

**What makes it recoverable is unrelated to the script:** the overwritten file
was committed, so `git checkout --` brought it back, and the work lost was the
uncommitted change the script existed to verify. That is the argument for
committing before running a harness that writes to the tree, not for writing a
better harness.

**The fix in the rewritten harness:** back up by full relative path, refuse to
start if two inputs are byte-identical, and assert the post-restore SHA-256 of
every file matches the pre-run one before exiting. The last of these is the one
that matters -- a restore that is not verified is not a restore, which is the
same `**absent != empty**` reasoning that an export you cannot read back is not
a backup.
