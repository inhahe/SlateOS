## TD-B-INSTALLS-BACKUP-RENAMES-TO-A-PATH-IT-INVENTED (lane B, 2026-09-11) -- FIXED 2026-09-11

**FIXED** with the `os_bytes`/`os_from_bytes` pair the entry named, factored
into `backup_name(dst, suffix)` so the reasoning has somewhere to live and the
contract has something to test.

**The part the entry did not anticipate, and it is about the tests rather than
the fix.** The defect only shows on a destination whose name is not valid
UTF-8, and `os_bytes` is documented as lossy on a Windows *host* -- which is
where this suite runs. So **none of the four new tests would fail against the
broken code**. They pin what makes the target behaviour right (the suffix is
appended to the whole name, byte for byte, never before an extension and never
normalised) and they would catch a rewrite that changed it, but they cannot
reach the case the function exists for.

That is stated in the test module rather than left for someone to discover,
because a suite that looks like it covers a fix and does not is worse than one
that admits the gap. The non-UTF-8 case is exercised by the target.


**In short:** `install --backup` moves the existing destination aside before
writing the new file. It builds the backup's name with

    let backup_path = format!("{}{}", dst.display(), args.backup_suffix);

`Path::display()` is **lossy**: any byte in `dst` that is not valid UTF-8 comes
out as U+FFFD. So on a destination whose name is not UTF-8, the rename targets
a path that is not `dst` plus a suffix — it is a *different name*, one the user
never had. The original file is moved somewhere they did not ask for and cannot
easily find, and `install` reports success.

**Why it is not hypothetical here.** This filesystem's rule is "any byte except
`/` and NUL", which is the whole reason CLAUDE.md item 7 exists. A name
arriving from a tarball, a foreign filesystem or a script is routinely not
UTF-8.

**Where:** `userspace/install/src/main.rs`, in the `--backup` path just above
the `fs::rename`.

**The fix** is to build the name as bytes rather than as text: take
`dst.as_os_str()`, append the suffix, and turn it back into an `OsString`
without ever going through `String`. `userspace/quoting` already exposes
`os_bytes` and `os_from_bytes` for exactly this round trip, and
`coreutils/src/bin/tar.rs` uses that pair.

**Found** while repairing the diagnostic on the very next line, which
interpolated the same `dst.display()` into a message. The message was the
flagged defect; the rename beside it is the one that moves a file.
