## 627. `diskquota` enforces its file-count limits, in the same call and the same grace period as its byte limits

**Date:** 2026-08-27
**Decided by:** Claude (autonomous)
**Lane:** A

**In short:** `diskquota` lets an administrator cap two things per user: how
many *bytes* they may store, and how many *files*. The byte cap worked. The
file cap was a prop — the shell accepted the numbers, stored them and printed a
confirmation, and nothing in the kernel ever compared anything against them, so
a user given a 200-file limit could create any number of files. This entry
records the decision to *enforce* the file cap rather than delete it, and three
smaller calls made along the way: that one check tests both caps rather than
two checks testing one each, that the answer says which cap was hit rather than
just yes/no, and that the two caps share one grace period.

Fixes `known-issues.md`
`A-DISKQUOTA-FILE-COUNT-LIMITS-ARE-STORED-AND-NEVER-COMPARED`.

### Why enforce rather than remove

Removing the fields was the cheaper end of the fork and would have been
defensible under §1 — never advertise a feature you do not honour. It is the
wrong call here for a reason specific to this module: **the counting half was
already real.** `update_usage` maintains `entry.file_count` on every call, and
has since it was written. So the kernel was already paying the cost of tracking
the number; only the comparison was missing. Deleting the limits would have
thrown away working machinery to avoid writing four lines of comparison, and
would have left `file_count` as a statistic nothing acts on — trading one
unhonoured feature for another.

The other direction of §1 also matters. `set_file_limits` returns
`KernelResult<()>` and reports `NotFound` when there is no such entry, so it
fails in exactly the situation a real implementation would. Nothing about the
interface admitted it was inert. That is the kind of feature §1 is aimed at,
and the honest repair is to make the promise true.

### One check for both caps, not one check per cap

`check_quota` gained a `files: u64` parameter rather than a sibling
`check_file_quota`.

*What changes:* a caller writes `check_quota(name, target, bytes, files)` once
instead of calling two functions and combining their answers.

The argument for two functions is that a write extending an existing file
creates no new file, so half the calls would pass `files = 0` — a parameter
that is usually zero looks like noise. The argument against, which wins, is
that **two functions invite a caller to check one and forget the other, and
that is precisely how this gap arose**: the byte path was written, the file
path was left for later, and nothing about the shape of the code forced anyone
back. A single entry point cannot be half-called. It also makes the verdict a
single consistent snapshot under one acquisition of the table lock; two calls
would sample the entry twice with a window between them.

### The answer names the limit that fired

`check_quota` returns `QuotaVerdict` — `Allowed`, `Warned { bytes, files }`, or
`Denied { bytes, files }` — instead of `bool`.

*What changes:* the shell prints `DENIED (hard file limit exceeded)` where it
used to print `DENIED`.

A denial that cannot say which cap it hit is close to useless to the person who
hits it: it sends someone hunting for a large file to delete when their actual
problem is two hundred small ones. The caller cannot recover the reason
afterwards, either — re-reading the entry to compare is a race against every
other writer and would describe the state *after* whatever happened next, not
the state the verdict was computed from. The cost is a wider return type and
one `match` at the single call site; the `bool` behaviour is preserved exactly
by `QuotaVerdict::allowed()`, which is what that call site uses for its
ALLOWED/DENIED word.

The `Warned` variant also makes an existing distinction visible. The old `bool`
returned `true` for both "under every limit" and "over a soft limit, permitted
with a warning" — a difference the module tracked internally
(`total_warnings`) but could not tell anyone about.

### `status()` returns the worse verdict, and ties go to bytes

*What changes:* a user at 400 files against a 200-file limit reads
`Hard Exceeded` instead of `OK`.

`QuotaEntry::status()` now scores each dimension with a shared
`dimension_status` helper and combines them by severity rank. The rank puts
`SoftExceeded` and `GracePeriod` **equal**, and ties resolve to the byte
verdict.

That tie-break is the deliberate part. `SoftExceeded` and `GracePeriod`
describe the same condition — over soft, under hard — and differ only in
whether the grace clock has started, which is a single per-entry fact rather
than a per-dimension one. Ranking them equal and preferring bytes on a tie
means the file half can only ever make a status *worse*, never merely
different at the same tier. Every existing reader of `status()` (kshell's `get`
and `list` arms) therefore sees no change unless the file count is genuinely
the worse of the two, which is the whole point of the change.

### One grace period, driven by the union of the soft limits

*What changes:* deleting a large file no longer resets a grace period that the
file count still justifies.

There is one `grace_start_ns` per entry, not one per dimension, so the clock
has to be driven by "over *either* soft limit" (`QuotaEntry::over_soft`) and
cleared only when both are back under. Keying it on bytes alone — the previous
code, unchanged — would have cleared the grace period the moment byte usage
dropped, while the user was still over their file cap with the clock reset to
never-started.

The alternative is two independent clocks, one per dimension. Rejected as
disproportionate: it doubles the state and the `QuotaStatus` vocabulary would
need to say *which* grace period, for a distinction no consumer asks about. If
the two ever need separate deadlines, the union condition is the right place to
split.

### What this makes live that was latent

`cmd_diskquota`'s `files` arm used to guess `0` for an unreadable limit word,
and `known-issues.md` recorded that as *latent* damage: a hard file limit
wrongly set to zero locked nobody out while nothing enforced it. It is active
now — a zero hard file limit denies the user's very next file. The
`required_num` refusals in that arm already prevent it. They were written on
2026-08-26, before the enforcement they now guard, and they were **verified
against the new behaviour rather than assumed to still fit**; the comment at the
site has been rewritten, since it explicitly described the fault as latent.

### How to reverse

Restore `check_quota`'s `-> KernelResult<bool>` and drop the `files` parameter;
restore `status()` to the byte-only comparison and `update_usage`'s grace
condition to `bytes_used >= soft_limit_bytes`; drop cases 9–14 from
`diskquota::self_test()` and the `[files]` argument from kshell's `check` arm.
The file limits would go back to being stored and never read. Nothing outside
this module and that one shell arm depends on any of it.
