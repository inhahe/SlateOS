## B-SSHD-LOGIN-GRACE-TIMER-COMPARED-MICROSECONDS-AGAINST-SECONDS (lane B) — FIXED 2026-09-05

**Status:** FIXED — `userspace/sshd/src/lib.rs`, commit on `lane-b` 2026-09-05.

**In short:** the SSH server disconnected every client during login, always,
with the message "login grace time expired". `LoginGraceTime` is the limit on
how long an unauthenticated connection may stay open — two minutes by default.
The code read the clock in *nanoseconds* and then converted as though it were
*milliseconds*, which left it comparing microseconds against a number of
seconds. Two minutes became a hundred and twenty **microseconds**, and a real
handshake takes several thousand times that. Nobody could log in.

### Where

`userspace/sshd/src/lib.rs`. The clock reader was called `clock_monotonic_ms`
and converted nothing — `SYS_CLOCK_MONOTONIC` is documented in
`kernel/src/syscall/number.rs` as "nanoseconds since boot", and the host-build
shim (`posix::syscall`'s `host_clock::monotonic_ns`) returns nanoseconds to
match. Its one caller, the authentication loop, then did

```rust
let elapsed_s = now.saturating_sub(start) / 1000;
if elapsed_s > u64::from(conn.config.login_grace_time) {
```

so `elapsed_s` held microseconds while `login_grace_time` held seconds (120 by
default, as OpenSSH's `LoginGraceTime` is).

### Why nothing caught it

The name. `clock_monotonic_ms` reads as a millisecond clock, and against a
millisecond clock `/ 1000` is exactly right — so the arithmetic looks correct at
the call site and the error is one identifier away, in a function whose body is
a single `try_from`. A unit that is asserted in a name and nowhere else is not
checked by anything.

Nor did the tests reach it. `sshd`'s own suite tests the pieces of
authentication (`decide_publickey_request`, the config parser, the password
path) rather than the loop that drives them, and the `ssh-interop` suite runs
the version and key exchanges but authenticates by calling the decision function
directly. The grace check sits in `handle_authentication`, which no test enters.
Nothing was wrong with any of those tests individually; the gap is that the one
loop containing the timer had no test at all.

### The fix

`clock_monotonic_ns`, named for what it returns, with the kernel's documentation
cited in its doc comment; a `NANOS_PER_SEC` constant; and the comparison itself
extracted into `login_grace_expired(start_ns, now_ns, grace_secs)`, which is
where the two units meet and is now the thing under test. The field
`connection_start_ms` became `connection_start_ns` for the same reason the
function was renamed.

The regression test — `the_login_grace_timer_measures_seconds_and_not_micro`
`seconds` — uses the numbers that actually occur rather than round ones: 10 ms
elapsed against a 120-second grace is the case every successful login passes
through, and it is the case that failed. It also pins the backwards-clock-step
behaviour, since that path is reachable by an unauthenticated peer.

### The general lesson

A unit carried only in an identifier is a comment that the compiler does not
read. Where a value crosses from one unit to another — a syscall's nanoseconds
into a configuration file's seconds — the conversion deserves to be a named
function with its own test, not an inline division whose correctness depends on
the reader remembering what the variable it divides is measured in.
