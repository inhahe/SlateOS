## §258 — A service asks the kernel who is calling, and the kernel answers with a snapshot

**Date:** 2026-08-21
**Decided by:** Claude (autonomous) — implementing `requests/b-a-a-service-cannot-find-out-who-is-calling-it.md`
**Lane:** A

**In short:** A program can connect to a named system service, but until now the
service had no way to find out *which* program had connected. So a service that
needs to decide "may this caller unlock someone else's screen?" could not answer
it. The only workaround is to have the caller state its own identity in the
message — which is asking the program you are trying to check to vouch for
itself. This adds one syscall, `SYS_CHANNEL_PEER_CRED` (number 286), where the
kernel reports the process ID, user ID and group ID of whoever is on the other
end of a connection. The number that gets reported is *recorded at the moment
the connection is made* and never looked up again afterwards, which is the part
of this decision with a real tradeoff.

### The problem

`SYS_SERVICE_ACCEPT` hands a service a channel and nothing else. Lane B's
`userspace/logind` implements `design-decisions.md` §341 — the desktop hands a
typed password to a privileged verifier and never sees a stored hash — and its
policy has three sentences, none of which can be evaluated without the caller's
uid:

- root may act on any session;
- anyone else may act only on their own, and someone else's session is reported
  as *not existing* rather than as *forbidden*, so the error is not a
  who-is-logged-in oracle;
- the password-free administrator override is root-only.

So `logind` currently answers `UnknownCaller` to every method and lane C's
`apps/lockscreen` cannot unlock a screen. Failing closed there is correct, and
it is why this was a missing feature rather than a security hole — but it is a
missing feature that blocks every privileged service we have not written yet,
each on its first authorisation check.

### The decision that had two defensible sides: snapshot vs. lookup

**(a) Resolve on read.** When a service calls `SYS_CHANNEL_PEER_CRED`, look up
the peer's process right then and report its current uid. Simpler — no new
state on the channel, nothing to keep in sync, and it reports the truth about
the process *as it is now*, which sounds like the more honest answer.

**(b) Snapshot at bind.** Record the peer's identity when its endpoint is bound
to a process — in `service::connect` for a client end, in the `accept` family
for a server end — and report that record forever after. Chosen.

The case for (b) is that (a) is not actually reporting the truth about the right
process. Three concrete failures:

1. **Process IDs are reused.** A client connects as pid 71, sends a request, and
   exits. Before the service handles it, pid 71 is issued to something else. A
   read-time lookup answers with the *new* process's uid, and the service
   authorises the wrong program. This is not a rare interleaving; it is what
   happens whenever a short-lived client outruns a busy service.
2. **A read-time answer can be "gone."** The peer exiting must not turn "who
   sent this?" into an error, or a service's behaviour depends on scheduling —
   the same request succeeds or fails depending on how quickly the client died.
   Under (b) the record outlives the peer for free, because `channel::close`
   removes a channel only once *both* sides have closed.
3. **Privilege changes apply retroactively under (a).** A process that drops
   privileges after connecting would retroactively lose the authority its
   connection was made with; worse, one that *gains* privileges would
   retroactively gain it on a channel it opened while unprivileged. Neither is
   what an authorisation decision means.

Linux snapshots `SO_PEERCRED` at `connect`/`socketpair` rather than resolving it
in `getsockopt`, for these reasons. The cost of (b) is 24 bytes per channel and
the obligation to record at every site that binds an endpoint — a real
maintenance burden, since a future bind site that forgets makes the credential
silently unavailable. That is mitigated by the direction of the failure: a
forgotten site reports *unknown*, and unknown is a refusal, so the bug is a
service that stops working rather than one that authorises a stranger.

### Three smaller calls, recorded because each could have gone the other way

**Unknown is not root.** `current_process_cred` returns `None` for a kernel task
rather than a uid-0 credential. A kernel-brokered connection genuinely has no
process behind it, and "the kernel did it, so allow it" is ambient authority
with a friendlier name. Every reader is documented to treat `None` as a refusal.

**A bound side cannot be rebound.** `set_side_cred` refuses rather than
overwrites. A credential that can be replaced is one the peer can influence,
which is the exact property the record exists to deny.

**"No such channel" and "no credential recorded" are one error.** Both return
`NotFound`. Distinguishing them would make the call report whether an arbitrary
handle value happens to name a live channel — a small oracle, bought for a
distinction no fail-closed caller can act on, since neither answer is a
credential.

### What is deliberately not covered

A channel pair from `SYS_CHANNEL_CREATE` records nothing, so `peer_cred` on it
reports unknown. Only the service registry brokers a connection between two
distinct processes; for a raw pair the kernel does not know which process will
end up holding which end, and a guess written into a credential field is worse
than an honest refusal.

The reserved fourth word is always zero, leaving room for the pidfd-style
generation counter lane B asked about without an ABI break. It is not populated
today because nothing needs it, and a field that ships with a real value is one
we must then keep meaning the same thing.
