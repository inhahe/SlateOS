### [B] D-POSIX-SOCKET-META-WAS-NOT-SCOPED-TO-ITS-FD-TABLE — ✅ FIXED 2026-08-14

**Found while running the eighth audit pass**, not by looking for it:
`socket::tests::test_phase201_bind_port443_no_cap_eacces` failed once with
`ENOTSOCK` where `EACCES` was expected, then passed three runs in a row.

`SOCKET_META` (posix/src/socket.rs) is indexed by fd number, so it must have
exactly the same scope as the fd table it is keyed by. `fdtable` made its
storage **per-thread** on host builds (design-decisions.md §110) precisely
because libtest runs tests on parallel threads. `SOCKET_META` stayed a
process-global `static mut`, and the mismatch was reachable: two tests on
different threads each create a socket and, drawing from *separate* per-thread
fd tables, both get the same fd number `N` — near-certain, not unlikely, since
each thread's table starts empty. They then shared one `SOCKET_META[N]`, and
the first to `close()` wiped the entry the other was still using, whose next
call saw a live fd with no metadata and reported `ENOTSOCK` for a good socket.

Fixed by giving `SOCKET_META` the same `cfg`-split storage as
`fdtable::fd_store`. Six consecutive full runs clean afterwards.

Two things worth keeping from this. First, the `// SAFETY: Single-threaded
access.` comments on these accesses were **true on the target and false under
`cargo test`** — a safety comment that silently changes truth value with
`cfg` is worse than none, and `fdtable` had already learned this lesson
without the fix being propagated to the table keyed by its own indices.
Second, an intermittent failure at roughly one run in four is easy to
dismiss as noise when it appears in a test unrelated to what you are
changing; it was worth the ten minutes to chase.
