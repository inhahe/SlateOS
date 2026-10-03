## B-TWO-TEST-FIXTURES-WERE-TESTING-THE-HOST-AND-NOT-THE-SUBJECT (lane B, 2026-08-31) — FIXED

**In short.** Eight tests were red on the Windows dev host and green on Linux.
None of them was a bug in the code being tested. Both causes were fixtures that
fed the subject a value **the target could never produce**, and then reported
the subject's correct handling of that impossible value as a defect.

**Class 1 — a fixture path joined with the host's separator (7 tests).**
`ScratchDir::path` used `PathBuf::join`, which on Windows inserts `\`. This OS
defines `/` as the only separator and every other byte — `\` included — as an
ordinary character in a file name, and `coreutils::pathname::is_slash` answers
only for `/`, correctly. So the fixture was handing the subject a
**single-component name that happened to contain backslashes**.

`backup.rs`'s numbered-backup scan derives the directory to read from the last
separator in the name it is given. Finding none, it scanned the *process's
current directory* instead of the scratch one, saw no `f.~1~` there, and
answered `f~`. Five `backup::tests` and two of `cp`'s failed on that, and each
looked like a backup bug.

Fixed in `ScratchDir::path`, which now appends `/` explicitly. Windows accepts
`/` in its own path APIs, so the path still opens on the dev host; only the
bytes change, and they change to the ones the target would emit. On Unix it is
byte-for-byte what `join` already did.

**Class 2 — a timestamp finer than the host clock (1 test).**
`both_sets_the_two_to_one_instant` asked for `UNIX_EPOCH + Duration::new(7, 8)`
— 8ns. A `SystemTime` is only as fine as the clock underneath it, and on
Windows that is a FILETIME at **100ns ticks**, so the value rounded to a flat
7s *before the subject ever saw it*. The assertion then read the host's dropped
nanosecond field as a defect in `Times::both`. `Times::both` carries `u32`
nanoseconds through unexamined and does not care which multiple it gets, so the
test now uses 800ns and proves the same property on both hosts.

**The generalisable point.** A cross-platform test suite has two subjects, and
only one of them is the code. When a test is red on one host and green on
another, the first question is not "what does the code do differently there" —
it is "**what did the fixture hand it that the target cannot produce**". Seven
of these eight were one line in a helper shared by six crates.

**Where.** `userspace/scratchdir/src/lib.rs` (`path`);
`userspace/coreutils/src/fsattr.rs` (`both_sets_the_two_to_one_instant`).
`cp`'s tests, which carried their own hand-rolled clock-named scratch helper
and a trailing `remove_dir_all` that an unwind skips, were converted to
`ScratchDir` in the same commit — 264 call sites, 121 cleanup tails removed.
