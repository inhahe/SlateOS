### B-ATFORK-TESTS-LEAKED-HANDLERS-INTO-A-SHARED-TABLE. `atfork_table_full_returns_enomem` failed intermittently under parallel test threads — 2026-08-13 — FIXED 2026-08-13

**Where:** `posix/src/pthread.rs`, tests `atfork_returns_zero` and
`atfork_with_handlers_returns_zero`.

**Root cause:** the atfork handler table is process-global, and the tests that
exercise ordering already serialise on `ATFORK_TEST_LOCK` and call
`atfork_reset()`. These two older tests did neither — they registered handlers
into the shared table and never removed them. `atfork_table_full_returns_enomem`
fills the table *exactly* and asserts every one of those registrations returns
`0`, so a leaked handler from a concurrently-running test made one of them
return `ENOMEM` (12) instead.

**Fix:** both tests now take `ATFORK_TEST_LOCK` and bracket themselves with
`atfork_reset()`, matching the convention of the other atfork tests. Unlike
`B-FTS-INSTANCE-POOL-CLAIM-IS-A-DATA-RACE` above, the production code here was
correct — `pthread_atfork` takes `ATFORK_LOCK` — so this was purely a test
isolation defect.

**Lesson:** when a module introduces a test-serialising lock for a global, every
test that touches that global has to take it; a partially-applied convention is
worse than none, because the failures land in the tests that *do* follow it.
