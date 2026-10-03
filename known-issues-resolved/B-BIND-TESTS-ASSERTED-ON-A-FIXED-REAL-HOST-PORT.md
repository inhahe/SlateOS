### B-BIND-TESTS-ASSERTED-ON-A-FIXED-REAL-HOST-PORT. `test_phase201_bind_port1024_no_cap_ok` failed whenever port 1024 was busy — 2026-08-13 — FIXED 2026-08-13

**Where:** `posix/src/socket.rs`, `test_phase201_bind_port1024_no_cap_ok` and
`test_phase201_bind_port8080_no_cap_ok`.

**Root cause:** both tests exist to prove one narrow property — that with
`CAP_NET_BIND_SERVICE` dropped, an *unprivileged* port is not rejected by the
privileged-port gate. But they asserted `bind(...) == 0`, and these binds reach
a real host socket on 127.0.0.1 at a *fixed* port. Any unrelated program
holding 1024 or 8080, or a socket from an earlier run of the suite still in
`TIME_WAIT`, makes the bind fail with `EADDRINUSE` and the test fail for a
reason unrelated to the code under test. Observed failing on one run out of
three consecutive suite invocations.

**Fix:** both now go through a shared `assert_bind_not_gated(port)` helper that
asserts the *absence of `EACCES`* rather than success. `EACCES` is the only
error the gate itself can raise, so this tests exactly the intended property and
is immune to host port availability. `test_phase201_bind_port0_no_cap_ok` keeps
its stronger `== 0` assertion because port 0 is ephemeral and cannot collide.

**Lesson:** assert on the specific failure mode under test, not on overall
success, whenever the call also depends on shared machine state.
