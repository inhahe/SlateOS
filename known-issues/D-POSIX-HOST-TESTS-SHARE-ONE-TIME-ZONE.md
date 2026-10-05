## D-POSIX-HOST-TESTS-SHARE-ONE-TIME-ZONE — `posix`'s host tests share one cached time zone, so a test that reads it without `TzGuard` races any test that sets one (lane D, 2026-10-05)

**Status:** OPEN — tech debt; each race found so far was fixed by taking the guard.

**In short:** the C library's tests run many at once, each on its own
thread of one process. Most per-process state in the library is kept per
test thread on the host (`posix/src/perprocess.rs`), so tests cannot see
each other's -- the environment among it since 2026-09-29. The time zone is
not: the zone a test sets is the zone every test running beside it sees. A
test that reads the zone (a local time, `mktime`, `strftime %s`) without
first setting one through `TzGuard` gets whatever zone a neighbour set, and
fails at random. The order gate (`check-test-order-independence.py`)
shuffles `posix` on every push from every lane, so such a race refuses
pushes at random -- lane B's on 2026-10-03, over
`time::tests::test_strftime_epoch_seconds`
(`requests/b-d-strftime-epoch-seconds-test-races-the-time-zone.md`).

**Where:** `posix/src/tz.rs` keeps glibc's statics as glibc does -- one
`State` behind `TZ_LOCK`, the name arena (`ARENA`, 8 KiB), the zone file
(`ZONE_FILE`, 16 KiB) -- and publishes `tzname`, `timezone` and `daylight`
to the C-visible statics in `posix/src/time.rs`. On the target that is
right: one process, one zone. On the host all of it is shared by every test
thread.

**Today's remedy, and why it is not enough:** every zone-reading test takes
`TzGuard` (set a zone, serialise on `lock_env_for_test`); 45 in `time.rs`
did, and the 46th did not, and nothing noticed until a push was refused.
The rule depends on each new test remembering it.

**The proper fix:** make the zone per test thread on the host, as the
environment is: `STATE`, `ARENA`, `ARENA_USED` and `ZONE_FILE` through
`process_global!` (26 KiB of thread-local storage, zeroed per test thread),
and the three C-visible variables behind host-only accessors, since on the
host they are not the C ABI's -- nothing outside the crate links the host
build. `TzGuard` then still sets a zone but no longer has to serialise, and
a test that forgets it reads the zone it was born with, UTC, instead of a
neighbour's. The target build does not change.
