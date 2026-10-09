### [A] dd-70's "leaf" premise is not occasionally wrong, it is systematically wrong: 1256 nested acquisitions per boot -- 2026-09-17

**Status:** OPEN -- fixed on lane-a-wip 2026-10-09 (the leaf-claim check reads zero), awaiting a boot on main.

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

**2026-10-02:** the pairs in `startmenu` -> `appregistry`, `columnview`,
`filetype`, `openwith` and `findex` are gone with those modules
(design-decisions 1528), so the next boot's count is the one to read.

**2026-10-02, later (on lane-a-wip):** rq39's leaf check named nine site pairs
on four outer locks, and all four are dealt with, two ways. Where the nesting
was needless it is gone and the lock stays a cheap true leaf: `bookmarks` and
`templates` kept their one-time-init flag as a lock of its own, held across the
store it filled; it is an `AtomicBool` read and set under the store's lock now.
`ipc::completion`'s `CP_TABLE` -- `try_lock`ed from the timer interrupt by
`try_notify`, so a conversion would have had to settle lockdep's view of
interrupt-context use first -- no longer wires an io_ring (`RING_TABLE`, then
`THRDOWN`) while held: `register` and `unregister` do that outside it, under a
new tracked `CP_WIRING` that serialises the two. Where the nesting is the design
the lock is converted: `ipc::unix_socket`'s `TABLE` takes `stream_socket`'s
`PAIRS` under it, is never taken in interrupt context and is no hot path, so it
is a `crate::sync::Mutex` (`UNIX_SOCKETS`) that lockdep now watches. rq39 hung
before the battery's end, so the count after these is the next full boot's.
From the older list, three more the same day: `clipboard`'s `CURRENT` and
`HISTORY` are one lock (`CLIPBOARD`); `dragdrop` copies its drop zones out
before taking the session's lock; `cgroupfs`' `STATE`, held across the
kernel's cgroup calls by design, is converted (`CGROUPFS`).

**2026-10-09 (lane-a-wip): the count is zero.** The full boot of that day
reported 476 acquisitions on 43 site pairs, every one of them under one of
fourteen outer locks. All fourteen are converted to `crate::sync::Mutex`,
which lockdep watches -- design-decisions 975's answer for a lock that has
another taken under it -- and named for its diagnostics:

| outer lock | what is taken under it | taken |
|---|---|---|
| `STATE` of `devhotplug`, `udriver`, `devpower`, `vmguest`, `initproc`, `reslimit`, `drvmon`, `svcstart`, `syshealth` | the event log's `EVENT_RING` (an event logged while the state is held); `svcstart` also `servicemgr`'s | when a device, driver, service or limit changes -- no hot path |
| `ipc::fifo`'s `FIFOS` | the pipe table's | when a FIFO is opened or forgotten |
| `proc::ptrace`'s `FPU` | `TRACEES`, by design -- the order a stop's way out takes them in | a debugger's register calls |
| `cnetwork`'s `TABLE` | the bridge's and the veth table's | when a container network changes |
| `termsession`'s `TABLE` | the lock a new session's set-up takes | session calls |
| `audio_mixer`'s `MIX_SCRATCH` | each stream's ring (`MIX_SCRATCH` -> ring, documented, never the reverse) | once per mixing period |

None is a hot path, so none needed the "revert by measurement" half of 975:
the tracking costs tens of nanoseconds on calls that come at most once per
mixing period. The next boot (lane-a-wip, one CPU, every self-test on):
`[sync] leaf-claim check: no lock was acquired inside a PreemptSpinMutex`,
lockdep's interrupt-context check clean (`0 violation(s), 0 suspect(s)`), no
ordering violation, BOOT_OK. What stays to be watched is the check itself:
a new nesting under a `PreemptSpinMutex` is named on the next boot.
