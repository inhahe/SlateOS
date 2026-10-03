## §289 — A tick that must not hold its own lock keeps its self-exclusion in a flag, and `net/ssh` checks the session *out* of the table rather than borrowing it in place

**Date:** 2026-08-23
**Decided by:** Claude (autonomous)
**Lane:** A

**In short:** two network servers (SSH and TFTP) had a once-a-tick housekeeping
function that grabbed one big lock at the top and let go at the bottom. That
lock was doing two different jobs at once: protecting the server's data, and
making sure two CPUs never ran the housekeeping at the same time. It also had
to be let go before the function reads a file from disk — holding it across a
file read is the deadlock this whole sweep was about. So the second job had to
move somewhere else, and for SSH the data could not simply be copied out and
copied back, because the protocol code needs to *keep writing to* one session
for the whole time. The decision is where each job went.

### The problem

`net/tftp.rs::server_tick` and `net/ssh.rs::tick` both opened with a single
`SERVER_STATE`/`STATE` lock and held it for their whole body. Underneath that
guard they reached the VFS — TFTP serves and stores files, SSH reads
`~/.ssh/authorized_keys` for public-key auth — which inverts the kernel's live
`filesystem lock -> module state` order (see `known-issues.md`,
`FIXED-A-THE-OPEN-FILE-TABLE-WAS-HELD-ACROSS-EVERY-FILE-READ`).

Releasing the lock around the VFS calls is the standard fix and is what the
other nine files in the sweep did. But it removes something neither tick was
getting deliberately: **mutual exclusion against another CPU running the same
tick**. Both have a rate limiter above the lock, and it looks like it provides
this, but it does not — `load`, compare, `store` is three operations, so two
CPUs can both read the same `last` and both proceed.

### Decision 1 — the self-exclusion moves to an explicit flag

A `SERVER_TICKING` / `TICKING` `AtomicBool`, claimed with
`compare_exchange(false, true)` and released by an RAII `TickGuard`, at the top
of the tick. A CPU that loses the race returns immediately rather than
spinning: the tick is periodic housekeeping, and the winner is about to do the
work anyway.

*Alternatives considered.*

- **Rely on the rate limiter.** Rejected: it is not atomic, as above. Making it
  atomic (a `compare_exchange` on `LAST_TICK`) would work, but it conflates
  "how often" with "how many at once", and reads as a rate limit to everyone
  who meets it later.
- **A second mutex covering the tick.** Works, but a mutex that is only ever
  `try_lock`ed is a flag with extra machinery, and it adds a lock to the order
  graph that lockdep then has to reason about.
- **Leave it unserialised.** Rejected. Two concurrent ticks would both drain
  the same socket and both drive the same session's state machine. It has
  probably never happened — the tick is called from one place — but the
  property was being provided, and silently dropping a property while fixing
  something else is how the *next* bug gets written.

*Cost.* The flag is a second thing to be right about, and a panic inside the
tick would leave it set forever if the guard were forgotten — which is why it
is an RAII `Drop`, not a manual store at the end.

### Decision 2 — SSH checks the session out of the table

TFTP's tick needed nothing more: its per-transfer work is a sequence of short
field updates, so it takes the lock in bursts and collects finished uploads
into a `Vec` to write after the guard drops.

SSH's does not fit that shape. `process_session` drives a protocol state
machine that holds one `&mut Session` across TCP reads, key exchange, decrypt,
`process_message`, and the response sends — with the VFS call in the middle of
it. There is no point where the borrow can be given up without unpicking the
state machine.

So the session is **moved out of its slot** with `mem::replace` and processed
on the tick's stack, then moved back. What is left behind is an empty
`Session` with `active: true` and a new `checked_out: true` flag.

*Why the placeholder, rather than just leaving the slot empty:* `active` is
what the accept path tests to find a free slot, so an empty slot would be
handed to a new connection and the write-back would then clobber it.

*Why `checked_out` on top of `active`:* `shutdown` walks the table and sends a
disconnect to each active session's `tcp_handle`. The placeholder's handle is
`0`, and `0` is a perfectly valid handle — belonging to somebody else. Without
the flag, shutting down during a tick would write an SSH disconnect packet into
an unrelated connection. `shutdown` skips checked-out slots and leaves them to
the tick, which sees `ENABLED` false on write-back and closes the real
connection itself.

*Alternatives considered.*

- **Per-session locks.** The cleanest answer in the abstract, and the one to
  reach for if sessions ever need to be processed concurrently. Rejected for
  now: it multiplies the lock-order graph by four for a table with four slots
  and one writer, and the checkout gives the same exclusion with no new lock.
- **Split `process_message` so the VFS work happens in the caller.** The
  authorization read depends on a username and public key that are only known
  once the message is parsed, so this means returning a "needs authorization"
  state and re-entering — a state machine on top of a state machine, in the
  one part of the code where being wrong is an authentication bypass.
- **Cache `authorized_keys` in memory.** Rejected outright: a revoked key would
  keep working until the cache expired.

*Cost.* A checked-out session is invisible to anything that inspects the table
by handle, and `stats()` can now observe the table mid-tick rather than only
between ticks. Both are acceptable — nothing looks a session up by handle, and
`active_session_count` still counts the placeholder — but they are the kind of
thing that stops being acceptable quietly, so the flag is named for what it is
rather than for what it is used for.
