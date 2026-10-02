### [A] dd-70's "leaf" premise is not occasionally wrong, it is systematically wrong: 1256 nested acquisitions per boot -- 2026-09-17

**Status:** OPEN

Boot `eb764a380`, with the reports deduped by site pair:

```
[sync] leaf-claim check: 1256 acquisition(s) inside a PreemptSpinMutex, 24 distinct site pair(s) named above (AT THE CAP -- there may be more)
```

dd-70 chose `PreemptSpinMutex` for "locks that never nest another lock
inside their critical section, so lockdep ordering checks add no value".
That sentence is the justification for the type not registering with
lockdep. It is false **1256 times per boot** across at least 24 distinct
site pairs, and the count is a floor: the pair table is full, which is why
the summary says so rather than going quiet.

The pairs split into two kinds.

**Cross-module, which are the ones worth reading:**

| outer | inner |
|---|---|
| `sockact.rs:202`, `:265`, `:419` | `eventlog.rs:746` |
| `fs/startmenu.rs:340`, `:414` | `fs/appregistry.rs:336` |
| `ipc/completion.rs:312`, `:364` | `ipc/io_ring.rs:744` |
| `ipc/completion.rs:312`, `:364` | `proc/thread.rs:853` (`THRDOWN`) |
| `fs/cgroupfs.rs:122` | `cgroup.rs:421`, `:676` (`CGROUP`) |

**Same-module**, dominated by one idiom: a one-time-init guard held across
the store it initialises -- `bookmarks.rs` (`INITIALIZED` -> `BOOKMARKS`),
`templates.rs`, `columnview.rs` (`INITIALIZED` -> `COLUMN_DEFS`) -- plus
adjacent-line pairs in `filetype.rs`, `openwith.rs`, `clipboard.rs`
(`CURRENT` -> `HISTORY`), `dragdrop.rs` (`ACTIVE_SESSION` -> `DROP_ZONES`)
and `findex.rs` (`INDEX` -> `FIELD_NAMES`).

**What this does and does not mean.** It is not a deadlock report. Nothing
here has been shown to form a cycle, and the ones inspected (`bookmarks`,
`templates`) are init-only with a single order. What it means is narrower and
worse: **the reason dd-70 gives for these locks being safe to leave out of
the validator is not the reason they are safe.** They are safe because no
pair happens to be taken in both orders -- which is precisely the property
lockdep exists to check, and which nothing checks for any of them.

That is `INOTIFY_TABLE` (fixed earlier today) at ~24x the scale, and it is
the same sentence `sync.rs` already wrote about the recursion case: "opting
out of one silently opted out of the other".

**A refinement the data exposes.** The dedup keys on (outer site, inner
site), but the interesting unit is the *lock* pair. `eventlog.rs:746` appears
with three different `sockact` outer sites: one lock pair, three site pairs.
So 24 site pairs is perhaps 12-15 distinct lock pairs, and the site view
inflates the apparent spread while being the more actionable one for a fix.
Keying on lock addresses as well would separate "how many orderings exist"
from "how many places create them"; not done.

**Why this goes to the operator rather than being fixed here.** Converting
the non-leaf ones to `crate::sync::Mutex` is the mechanical fix and it costs
per-acquire tracking on paths dd-70 specifically chose the cheap type for --
on measurements that, as recorded above, were never taken for this type until
today. 489 instances, a cost/correctness trade, and a decision that is
already written down: that is `open-questions.md`, not a unilateral sweep.
