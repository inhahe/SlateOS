### [A] Renaming a file silently drops its immutable protection, and the self-test that covers it calls the fixup nothing else calls -- 2026-09-21
**Status:** OPEN. Worse than the hard-link gap I filed for this table, and independent of the FileId fix.

**In short:** a file can be marked immutable so writes, deletes and renames
are refused. The table remembers the file by name. There is a function whose
job is to move the entry when a file is renamed, it works, it is tested --
and nothing in the filesystem calls it. So renaming a protected file leaves
the protection attached to a name nothing uses, and the file becomes
writable.

**The three facts, each measured:**

| fact | evidence |
|---|---|
| the fixup exists and is correct | `immutable::rename_path(old, new)` at `immutable.rs:347` moves the flag entry |
| it is tested | the module self-test calls it at `:497` and `:536`, including a non-UTF-8 name, and asserts the move |
| **nothing else calls it** | `grep -rn 'immutable::rename_path'` across `kernel/src` returns only the definition and those two test calls |

So the protection is lost on every rename, and the test reports the opposite.

**Why this is the sharpest instance of today's shape.** The other cases in
this family are quiet: a counter that can only read zero, a module reachable
only from `kshell`. Here the test does not merely fail to catch the defect --
it **calls the missing link directly**, so it exercises the one path
production does not take. A reader seeing `immutable::self_test ... OK` has
been told that renames preserve flags. They do not.

**The fix is to delete the fixup, not to wire it up.** Keying the table on
`FileId` -- the change already made to `flock`, `sealing` and `reclock` today
-- makes `rename_path` unnecessary: an inode survives a rename, so the entry
follows the file with no compensation at all. Wiring the fixup into the VFS
rename path would work and would be the worse repair: it leaves a table that
needs a correction on every operation that moves a name, and the next such
operation (link, mount-move) needs another.

**Two things make that conversion bigger than the other three**, which is why
it is filed rather than done in the same pass:

* `list_flagged() -> Vec<(PathBuf, FlagBits)>` puts the key type in a public
  signature, with callers in `procfs.rs:5604` and `kshell.rs:112681`.
* the table is a `BTreeMap<PathBuf, FlagBits>` across 14 access points, so the
  key becomes an enum (`Id(FileId) | Path(PathBuf)`) needing an `Ord` derive.

**Until then the honest interim** is a one-line note on `rename_path` saying
no caller exists and flags do not survive a rename -- so the next reader of
that function is not misled by its own test the way I nearly was.
