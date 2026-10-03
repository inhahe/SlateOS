## `A-DISKQUOTA-FILE-COUNT-LIMITS-ARE-STORED-AND-NEVER-COMPARED` (lane A, 2026-08-26) — **FIXED 2026-08-27**, tech debt

**In short:** `diskquota` can limit two things: how many *bytes* a user may
store, and how many *files*. The byte half works. The file half is a prop —
`diskquota files alice user 100 200` accepts the numbers, stores them, prints a
confirmation, and nothing anywhere ever looks at them again. A user given a
200-file limit can create any number of files.

**Where.** `kernel/src/fs/diskquota.rs`. The fields exist
(`QuotaEntry::soft_limit_files`, `hard_limit_files`, lines ~83–84) and are
written in exactly two places — the `u64::MAX` seed for a newly created entry
(~195–196) and `set_file_limits` (~218–219). A tree-wide grep for
`limit_files` returns no reader outside this file, and inside it there is none
either: `QuotaEntry::status()` compares `bytes_used` against the byte limits
only, and `check_quota` likewise tests `new_usage > entry.hard_limit_bytes`.

The usage side is *also* live and equally unconsulted: `update_usage` maintains
`entry.file_count` (~272–274), so the kernel dutifully tracks a running file
count and dutifully stores a ceiling for it, and never once compares the two.

**Why it is worth an entry rather than a shrug.** This is not a stub that
announces itself. `set_file_limits` returns `KernelResult<()>` and reports
`NotFound` when there is no such quota entry, so it fails in exactly the
situation a real implementation would, which makes it look implemented. The
shell prints `File limits for user 'alice': soft=100 hard=200` — a success line
naming the numbers. Everything about the interface says the limit took effect.

**The proper fix** is to enforce it, not to remove it: extend `status()` to
return the worse of the byte and file verdicts, and give `check_quota` a
file-count counterpart (or a `files: u64` parameter) so a write that would
create a file is tested against `hard_limit_files` the way its bytes are tested
against `hard_limit_bytes`. The grace-period machinery already keyed off the
soft byte limit should apply to the soft file limit on the same terms.

**Until then, do not let the D1 burn-down overstate the damage in this arm.**
When `cmd_diskquota`'s guessed-value sites are fixed, the `files` subcommand's
guessed `0` is a *latent* fault, not an active one: it records a limit that is
wrong and that nothing currently enforces. It becomes an active lockout the
moment the enforcement above is written. The `set` arm's guessed `0` is a live
lockout today, because the byte path is real.

**Not a regression.** True since `set_file_limits` was written.

**Fixed 2026-08-27 (lane A), as the entry above prescribed.** The file half is
now compared everywhere the byte half is:

- `QuotaEntry::status()` returns the **worse** of the byte and file verdicts.
  The two dimensions are scored by a shared `dimension_status` helper and
  combined by severity rank, with `SoftExceeded` and `GracePeriod` ranking
  equal and ties going to the byte verdict — so the file half can only ever
  make a status *worse*, never merely different at the same tier. Every
  existing `status()` reader (kshell's `get` and `list` arms) is therefore
  unchanged unless the file count is genuinely the worse of the two.
- `check_quota` takes a `files: u64` parameter and tests both limits in one
  call under one lock acquisition. A separate `check_file_quota` was rejected:
  two functions invite a caller to check one and forget the other, which is
  exactly how this gap arose.
- It returns `QuotaVerdict` (`Allowed` / `Warned{bytes,files}` /
  `Denied{bytes,files}`) rather than `bool`. A denial that cannot say which
  limit fired sends the user hunting for a large file to delete when their
  problem is two hundred small ones, and the caller cannot reconstruct the
  reason afterwards without a racy re-read.
- The grace clock is driven by the **union** of the two soft limits
  (`QuotaEntry::over_soft`). There is one `grace_start_ns` per entry, so
  keying it on bytes alone would have cleared a grace period the file count
  still justified the moment the user deleted a large file.
- kshell's `check` arm gains an optional `[files]` argument, given the §607
  treatment `update` already gives `[file_delta]`: absent means the honest
  default (a write extending an existing file creates none), present-but-
  unreadable is refused. Its output now names the limit that fired.
- `diskquota::self_test()` grew from 8 cases to 14. The six new ones use an
  entry with `u64::MAX` byte limits so every verdict is attributable to the
  file half alone: allowed under both, warned over soft, **denied over hard**
  (the regression the change exists for), grace started by the file count with
  zero bytes used, `status()` reporting `HardExceeded` while the byte half is
  `Ok`, and the grace clock clearing when the count drops back under.

**The latent/active note above is now spent.** `cmd_diskquota`'s `files` arm
guessed `0` is no longer latent — a hard file limit silently guessed as zero
would deny the user's very next file. The `required_num` refusals in that arm
already prevent it; they were written before the enforcement they now guard,
and were verified against it rather than assumed. The comment at the site has
been updated to say so.
