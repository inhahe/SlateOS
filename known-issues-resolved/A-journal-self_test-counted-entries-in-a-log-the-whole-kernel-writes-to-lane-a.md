## `journal::self_test` counted entries in a log the whole kernel writes to (lane A)

**Status:** FIXED 2026-08-22 (lane A), in the same commit that un-gated the
suite — see *"Seven of the eight boot self-tests behind `if fat_ok`"* above.
Unlike the `notify` case, this one was fixed *before* it produced a failure.

**In short:** The change journal is a single running list of "what changed on
disk", and every file the kernel creates, writes, deletes or renames adds a line
to it. The journal's self-test recorded four changes of its own and then checked
the list contained exactly those. That only holds if nothing else writes a file
while the test runs. Nothing else did, as long as the test was skipped on the
automated boot; the moment it was allowed to run there, the list already had 191
entries in it from ordinary boot activity.

### Evidence

The first boot that ran the un-gated suite:

```
[journal] Running self-test...
[journal]   Start cursor: 191      <- 191 entries already recorded, by the rest of the kernel
```

and `grep -rn "journal::record" kernel/src` shows why: `fs/vfs.rs` records on
create, write, delete, rename, truncate and more. The suite is one writer among
many, not the only one.

### Root cause

Three assertions assumed sole ownership of the log:

1. `entries.len() < 4` followed by `&entries[entries.len() - 4..]` — tolerant of
   foreign entries *before* its four, but not of one landing *between* them.
2. `if cursor() != before` — "recording internal metadata consumed no sequence
   number". A concurrent VFS write legitimately consumes one, so this is a false
   failure waiting for the first bit of real traffic.
3. `entries.len() != 1` after recording `/_JOURNAL.bak` — any foreign entry in
   the same window breaks it.

### Fix

- Probe paths carry a `JOURNAL_ST_` marker and the counting assertions filter to
  it via `probe_entries_since()`. The marker is matched as a **substring, not a
  prefix**, because one case deliberately records a path beginning with
  `JOURNAL_FILE` (`/_JOURNAL_ST_NEARMISS.bak`) to prove the internal-metadata
  exclusion is an exact match rather than a prefix rule. Moving that path under
  a prefix of its own would have deleted the case's meaning.
- Assertion 1 tightened from `>= 4` to **exactly 4**, which filtering makes both
  meaningful and safe.
- Assertion 2 restated. "The cursor did not move" is only true on a quiet
  filesystem; "every seq consumed since `before` is accounted for by an entry we
  can see" (`cursor() == before + all_entries.len()`) is robust to foreign
  traffic *and strictly stronger* — a record that burns a sequence number
  without producing a visible entry is exactly the bug under test, no matter who
  made it. Note this deliberately reads the log **unfiltered**: the invariant is
  about the whole log, and filtering would break it.

### Why this is worth its own entry

The fix for the `notify` sibling was a filter. Here a filter alone would have
been wrong: assertion 2 is a statement about global state and cannot be filtered
without becoming vacuous. Two suites, the same root cause, two different correct
fixes — the question to ask is not "how do I ignore other writers" but "what is
this assertion actually claiming, and is that claim still true when the system
is busy".
