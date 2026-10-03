## TD-B-CURL-MAX-TIME-IS-CHECKED-BETWEEN-READS-NOT-DURING-ONE — 2026-09-15 — OPEN

**In short:** `curl --max-time 30` now works, but only at read boundaries. A
server that accepts the connection and then stalls *inside* a single read still
hangs forever, because `SYS_TCP_RECV` blocks in the kernel with no timeout of
its own and nothing can interrupt it from userspace.

**What was fixed and what was not.** Both `--connect-timeout` and `--max-time`
were parsed, range-checked, stored in `Options` and read by nothing — found by
`check-fields-written-never-read.py --advertised`, which ranks a field by
whether the program's own `--help` promises it. Both are honoured now:

* `--connect-timeout` is exact. `SYS_TCP_CONNECT` has always taken a flags word
  in `arg2` whose bit 0 asks for a non-blocking connect; curl passed a
  hard-coded `0`. It now takes that path when a limit is set and polls
  `SYS_TCP_INFO`'s state byte for the handshake, so the timeout is enforced to
  within the 5 ms poll interval.
* `--max-time` is **coarse**. It is checked before each `tcp_recv` in both the
  header and body loops, which bounds a server that dribbles data or stalls
  between records — the case that actually happens — and does not bound one
  that goes silent mid-read.

**What the proper fix looks like.** A recv timeout in the kernel, i.e. an
`arg3` on `SYS_TCP_RECV` carrying a deadline in milliseconds, mirroring what
`arg2` already does for `SYS_TCP_CONNECT`. That is lane A's tree. It has not
been filed as a request yet because the coarse bound covers the observed
failure mode and a syscall ABI change is worth more than that buys — the next
lane to want a recv timeout for its own reasons should carry it.

**Why this is not the shape it replaces.** The option no longer lies: it does
something, and where it stops is written down here and in `recv_headers`'
comment rather than left for a user to discover. An inert option is defensible
only while nothing promises otherwise; a *partial* one is defensible when the
partiality is stated.

**Where it lives:** `userspace/curl/src/main.rs` — `Deadline`,
`tcp_connect_timeout`, `tcp_state`, and the two `deadline.check()?` calls.
