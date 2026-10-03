### FIXED-A-LOGPERSIST-NEVER-PERSISTED-THE-SYSTEMS-FIRST-EVENT (lane A)

**In short:** The kernel keeps a log of notable events and periodically
writes new ones out to a file on disk. It remembered how far it had got by
storing the number of the last event it wrote, and it started that counter
at zero to mean "haven't written anything yet". But events are also numbered
from zero — so the very first event the system ever recorded had the same
number as the "nothing yet" marker, and the writer skipped it as already
done. On every fresh boot, event number one was silently lost. It is the
event most worth having: if a machine dies seconds into booting, that may be
the only one it managed to record. Fixed.

**Status:** FIXED 2026-08-23, commit `b66622ab6`.

**Where:** `kernel/src/logpersist.rs` — `State::global_last_flushed`,
`FlushCursor::last_flushed_seq`, and the two `EventFilter::after(..)` calls
in `flush()`.

**The defect.** `eventlog` assigns sequence numbers starting at 0
(`kernel/src/eventlog.rs:647`, `let seq = self.total_written;`). `flush()`
resumed from where it left off with `EventFilter::all().after(after_seq)`,
and `after` is *strictly* greater (`eventlog.rs:916`, `if entry.seq <= after
{ return false }`). With `global_last_flushed: u64` seeded to `0` to mean
"nothing flushed yet", the first flush of every boot asked for "everything
after 0" and thereby excluded sequence 0.

This is the in-band-sentinel class again (see
`FIXED-A-SYSMAINT-NEVER-RUN-TASKS-WERE-NEVER-DUE` and the `tasksched`
`last_run_ns` fix): a `u64` field where one legal value is stolen to mean
"unset". The distinguishing feature here is that **no** `u64` could have
worked — `after(n)` has no value meaning "everything", because the range it
excludes always includes at least sequence 0.

**The fix.** `Option<u64>` for both cursors, and the first flush omits the
`after` clause rather than passing a sentinel:

```rust
let base = EventFilter::all().min_severity(min_sev);
let filter = match after_seq {
    None => base,
    Some(seq) => base.after(seq),
};
```

Both display sites (`logpersist::format_stats` and `kshell`'s `logrotate
stats`) now print `none (nothing flushed yet)` rather than `0`, which had
claimed a flush that never happened.

**How it was found.** logpersist's own self-test, which reported
`expected 2 flushed, got 1` on boot batch 29 — the first boot after the
suite was de-fanged and actually started running. This is the fourth
product bug the de-fanging programme has surfaced, and the pattern holds:
the suites were switched off *because* they failed, and they failed
because they were right.

The test now pins the cause rather than the symptom — it asserts that the
post-`clear()` event really is sequence 0, that both events flush, that the
resulting cursor is `Some(1)`, and that a freshly-initialised cursor reads
`None` rather than `Some(0)`.
