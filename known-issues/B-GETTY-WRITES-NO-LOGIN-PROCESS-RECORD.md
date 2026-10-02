## B-GETTY-WRITES-NO-LOGIN-PROCESS-RECORD — `getty` does not put its terminal in `utmp` while it waits for a name (lane B, 2026-10-01) — **Status: OPEN (debt)**

**In short:** on Linux, `agetty` writes a `LOGIN_PROCESS` record for its
terminal when it starts, so `who -l` (and `w`) can show which terminals are
waiting for a login, and `login` finds that record and turns it into the
user's session, keeping its id. Ours writes nothing; `login`
(`userspace/login/src/records.rs`, `log_utmp`) handles both cases, making a
new record when there is none, so nothing is lost but the waiting
terminals' lines.

**Where:** `userspace/getty/src/main.rs`; the call to copy is util-linux
2.39.3's `term-utils/agetty.c`, `update_utmp` -- `pututxline` of a
`LOGIN_PROCESS` record and `updwtmpx` to `wtmp`, through `libcall::utmp`.
