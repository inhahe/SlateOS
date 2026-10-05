## TD-B-HARDLINK-MERGES-ON-CONTENT-ALONE-AND-ALL-FIVE-RESPECT-FLAGS-ARE-INERT — 2026-09-15 — FIXED same day

**In short:** `hardlink` decides two files are the same from their **contents
only**. Every flag that exists to narrow that — `-f/--respect-name`,
`-t/--respect-time`, `-p/--respect-perm`, `-o/--respect-owner`,
`-x/--respect-xattr` — is parsed, stored, advertised, and read by nothing. A
user who passes `-o` to avoid merging across owners gets the merge anyway.

**Why it matters more than an ordinary inert flag.** Linking is destructive and
collapses metadata. Two files with identical bytes but different modes become
one inode with the *master's* mode, so `hardlink -p` failing to respect
permissions can turn a `0600` file into a `0644` one — a privacy regression
the user explicitly asked to prevent. `-o` does the same for ownership.

**What implementing them needs.** `FileInfo` carries the path and size;
deciding these flags needs `st_mode`, `st_uid`, `st_gid`, `st_mtime` and the
xattr set captured at scan time, then compared before a group is linked rather
than after. That is a change to what the scan records, not a condition bolted
onto the link step — the grouping happens by content hash long before
`link_over` is reached, so filtering at the link is too late to be cheap and
too early to be correct.

**Not fixed in the same change as the data-loss repair**, deliberately: that
commit's claim is "a failed link no longer destroys the duplicate", and
widening it to "and the right files are chosen" would make one commit answer
two questions. The destructive window was the urgent half.

**FIXED in the commit after it.** `FileInfo` now records real `mtime`, `mode`,
`uid` and `gid` as `Option`s, and `may_link` consults them before any pair is
merged. The four `_`-prefixed fields it replaces were hardcoded to `0` with
the comment "Platform-dependent, simulated" -- **and that is why wiring the
flags to them would have been worse than leaving them inert.** A zero standing
in for a real mode compares equal to every other zero, so every pair would have
passed every check and the flags would have looked implemented while preventing
nothing.

So an attribute this build cannot read is an `Err`, never a match:
`--respect-perm` on a platform with no mode bits refuses the merge rather than
permitting it. `--respect-xattr` always refuses, because there is no
`getxattr` to call. `--respect-name` compares the BASENAME, not the path --
deduplicating identical files across directories is the point of the tool.

**Where it lives:** `userspace/hardlink/src/main.rs` — `HardlinkOpts`'s five
`respect_*` fields, `files_identical`, and the grouping in `deduplicate`.
