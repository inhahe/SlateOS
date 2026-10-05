### [F] On SlateOS the compositor still asks its listener for connections every frame, because `poll` never reports one waiting -- 2026-09-25

**Status: FIXED 2026-09-26** — lane A's `851d9165b` makes the network daemon
answer `OP_POLL` for a listener (readable when a connection is waiting), and
lane F then removed the workaround: `LISTENER_READINESS` is gone, and with it
`listener_worth_asking` and `accept_deadline`, so on SlateOS too the compositor
sleeps until a connection or a request wakes it. The original report follows.

**In short:** the compositor now sleeps until there is something to do instead
of waking sixty times a second to look (design-decisions §1302). On SlateOS it
cannot yet sleep all the way, because the kernel never tells a waiting program
that someone has connected to it. So there it still wakes once a frame to ask —
exactly as often as before, no worse — and an idle SlateOS desktop keeps its old
wakeup rate until lane A's fix lands.

**Where it bites.** `gui/remote/src/wait.rs`, `LISTENER_READINESS`, which is
`false` when `target_vendor = "slateos"`; `gui/compositor/src/server.rs`,
`listener_worth_asking` and `accept_deadline`, which turn that into "ask every
tick, wait at most one frame".

**Why.** `kernel/src/net/socket.rs::poll_ready` answers for a listening socket
by sending `OP_POLL` for the listener id to the network daemon;
`services/netstack`'s `ring_tcp_poll` looks the id up only among connections and
returns `-1`, which the kernel reads as "nothing waiting". A server that trusted
the wait would never accept anyone — every application launched after the
compositor had started would hang waiting for its window.

**The proper fix** is lane A's: the daemon answers `OP_POLL` for a listener
("readable iff an established connection is in the backlog"). Lane F's side is
then one line — `LISTENER_READINESS` becomes `true`, or goes — and the two
helpers with it.

**How to see it.** Not observable as a fault on SlateOS today, because the
workaround is in place; `a_listener_the_platform_cannot_vouch_for_is_asked_every_frame`
holds the workaround's two halves. The kernel side is visible directly: `poll`
a listening socket with a connection pending and `revents` comes back 0.
