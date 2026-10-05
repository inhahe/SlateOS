## B-A-EVDEV-SYN-DROPPED-ARRIVES-ONE-RECORD-LATE (lane A, 2026-08-22) — FIXED 2026-08-22

**In short:** when a program reading the keyboard or mouse falls so far behind
that the kernel has to throw away events it never delivered, the kernel is
supposed to tell it "your picture of the device is stale — ask the device what
state it is in and start again from here." That message was being sent one event
too late: the program received one real event *first*, and only then the
warning. So it would apply that event to the very state it was about to be told
to discard — a key that stays stuck down, or a mouse button that latches the
wrong way, for as long as the program runs.

**Where:** `kernel/src/evdev.rs`, `EvdevClient::next_event`. The function
answered an owed `SYN_DROPPED` before calling `resync()`, so a lap discovered on
that same call set the flag *after* it had already been tested.

**Fix:** `resync()` moved to the top of the retry loop, ahead of both the
pending-marker test and the caught-up test. The caught-up test has to stay below
the marker test, because a grab released via `discard_pending()` leaves the
cursor level with the head with a marker still owed, and that read must return
the marker rather than "nothing available". Commit `de5e9743b`.

**How it was found, and why it took a boot to find it:** `evdev::self_test`
existed in `46e69a1c1` but was only wired into `main.rs` as a fatal check in
`f37616f4c`, so the first boot that actually ran it failed hard —
`[evdev] SELF-TEST FAILED: ...specifically SYN_DROPPED`. The test was itself
weak enough to have nearly missed it: it asserted the first record's type and
its code in two separate `check!`s, and `EV_SYN` is 0, so the trailing
`SYN_REPORT` of the resumed group satisfied the type assertion on its own. Both
are now asserted as a single four-byte pair, and the second record is pinned to
the oldest event the ring still holds (derived from `RING_CAP` and the
three-events-per-press shape, not hardcoded).

**Lesson worth keeping:** a self-test that is written but not *called* is not a
test. Grep for `self_test` functions with no caller before trusting a "tested"
claim in a commit message.
