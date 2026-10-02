## B-AUTH-DAEMON-RATE-LIMIT-TESTS-RACE-A-ONE-SECOND-WINDOW (lane B, filed by lane C 2026-08-21) — OPEN, filed as a request

**In short:** four userspace daemons test their password rate limiter by making
four wrong guesses and asserting the fifth is refused. The refusal lasts exactly
one *real* second, and the four guesses each run a real password hash, so on a
loaded machine the window is already over by the time the fifth guess happens
and the test fails. Observed once in a lane-C full-workspace run.

**Where it lives:** `userspace/sshd/src/main.rs` (observed),
`userspace/ftpd/src/main.rs`, `userspace/doas/src/main.rs`,
`userspace/logind/src/main.rs` — all four build the authenticator with the real
clock. `userspace/authlib/src/lib.rs` ~line 520 is the window
(`now < last_failure_secs + delay_for(failures)`; `delay_for(4) == 1`).

**Reproduce:** `cargo test --workspace --target x86_64-pc-windows-gnu` under
load. Green in isolation (`sshd` 140/140). Observed:
`sshd/src/main.rs:4902: expected a rate limit, got Rejected`.

**Proper fix:** pin the clock, as `authlib`'s own copy of the same test already
does — `Authenticator::with_clock(...)` over an `AtomicU64` the test steps
deliberately. This also strengthens the assertions: a frozen clock lets them
check `RateLimited { retry_after_secs: 1 }` at a known instant instead of
`matches!(.., RateLimited { .. })` at whatever instant the scheduler provided.

**Why it matters more than a flake:** the failing direction is noisy but the
*passing* direction is quietly weak. A pass proves only "refused within a second
of the fourth failure", which a mis-computed delay would also satisfy. The tests
are worth less than their names claim even when green.

**Not lane C's tree.** Filed as
`requests/c-b-auth-daemon-rate-limit-tests-race-a-one-second-window.md` with the
full diagnosis. Distinct from the resolved
`B-FTPD-SSHD-AUTH-TESTS-SHARE-TEMP-FILES-AND-FLAKE` — that was shared temp
files and lane B's `ScratchDir` fix holds; this is wall-clock timing.
