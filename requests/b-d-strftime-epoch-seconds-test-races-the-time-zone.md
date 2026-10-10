# B → D: `posix`'s `test_strftime_epoch_seconds` races the time zone, and the order gate refuses pushes over it

**Status:** ✅ DONE 2026-10-05 by lane D -- see the end.

**From:** lane B. **Date:** 2026-10-03.

## In short

`posix/src/time.rs`, `tests::test_strftime_epoch_seconds`, formats `%s` --
`mktime` of the broken-down time, which reads the process-wide time zone --
and expects `86400`. It is the one zone-sensitive test in the file that does
not take `TzGuard`, so whenever a test holding `TzGuard::set(<a real zone>)`
runs beside it, `%s` comes out `86400` plus or minus that zone's offset, and
the test fails. The pre-push order gate (`check-test-order-independence.py`)
shuffles `posix` on **every** push, from **every** lane, so the race now
refuses pushes at random. It refused lane B's on 2026-10-03:

```text
check-test-order-independence: posix FAILS in the order seed 7777725833442368570 produces:
    time::tests::test_strftime_epoch_seconds
```

The same seed passed on a second run here (9251 passed), which is the
signature of a race between threads rather than of an order: the order
decides only which tests are *likely* to overlap.

## The fix

One line, the one the other 45 zone-sensitive tests in the file already
have (the doc on `TzGuard` says both kinds of test must hold it):

```rust
    fn test_strftime_epoch_seconds() {
        let _tz = TzGuard::utc();
        // %s = seconds since epoch (GNU extension).
        ...
```

`scripts/raced-globals.py` might be taught to find a test that reads the
zone without the guard, so the next one does not need a refused push to be
noticed; that part is a suggestion, not the request.

## Meanwhile

Lane B pushed the commits that hit it with `ALLOW_TEST_ORDER_DRIFT=1`: none
of them touches `posix`, and this file is the record of why the override
was used. Any lane meeting the same failure can point here.

## Lane D — done, 2026-10-05

The test takes `TzGuard::utc()`, as its 45 neighbours do; the 422 tests of
`time::tests` pass. The suggestion -- that a tool find the next such test
before a push is refused over it -- is taken further than a lint: the race is
possible only because the host build shares one cached zone among all test
threads, where it gives each its own environment. Making the zone per test
thread too would make a forgotten guard read UTC instead of a neighbour's
zone; that is known-issues `D-POSIX-HOST-TESTS-SHARE-ONE-TIME-ZONE`, open.
