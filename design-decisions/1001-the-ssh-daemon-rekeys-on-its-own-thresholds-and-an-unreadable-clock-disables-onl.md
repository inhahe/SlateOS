## 1001. The SSH daemon rekeys on its own thresholds, and an unreadable clock disables only half of them

**Date:** 2026-09-05
**Lane:** B
**Decided by:** Claude (autonomous)

**In short:** an SSH connection is supposed to throw away its encryption keys
and negotiate fresh ones periodically — after about an hour, or after about a
gigabyte of traffic, whichever comes first. Ours never did, and it also ignored
the client when the client asked, which did not merely skip the refresh: it
**froze the session**, because a peer that has asked for new keys stops sending
anything else until it gets them. The daemon now asks on its own schedule, and
the client answers. Two smaller choices inside that are written down here: what
to do when the clock cannot be read, and how much of the user's typing to hold
in memory while a rekey is in flight.

**Jargon, once.** *Rekey* — renegotiating the session's encryption keys without
dropping the connection. *KEXINIT* — the message that starts that negotiation;
either end may send it. *Session identifier* — a fingerprint of the *first*
negotiation, fixed for the life of the connection and used as the thing a
public-key login signs over; it must not move when the keys do.

### Who decides when to rekey

| Option | *What changes:* | Why not |
|---|---|---|
| **The daemon has its own thresholds** (chosen) | A session that runs an hour, or moves a gibibyte, renegotiates without anyone asking. | Costs a round trip on a timer, and needs code at both ends rather than one. |
| Only answer the client's requests | Nothing, against a client that asks; against one that never does, one set of keys protects an unbounded amount of traffic forever. | Makes the security of *our* server a property of whoever connected to it. A client we did not write, or one deliberately built not to ask, sets our key lifetime. |

Answering the client is not optional either — that half is the actual bug
(`known-issues.md`
`TD-B-SSHD-DOES-NOT-REKEY-SO-A-LONG-SESSION-HANGS`), and it shipped first. The
decision recorded here is the second half.

### An unreadable clock, and why this is the *opposite* call from §775

The age threshold needs the monotonic clock, which can fail to read. §775 chose
to **fail closed** in that situation — hang up. This one fails **open**: if the
clock cannot be read, the age threshold simply does not fire, and the byte
counter, which is not a clock, goes on working.

| Option | *What changes:* | Why not |
|---|---|---|
| **Fail open — no clock, no age threshold** (chosen) | A session whose clock reads are failing keeps running, protected by the gibibyte limit alone. | The hour-based refresh silently stops happening, and nothing reports it. |
| Fail closed — drop the session | A transient clock failure disconnects users in the middle of their work. | Spends something real (the user's session, and whatever it was transferring) to enforce something the RFC calls a recommendation. |

The two entries look inconsistent and are not. §775's timer bounds a peer who
**has not proved who they are**: the whole point of it is to stop an
unauthenticated stranger holding a connection, so a version of it that quietly
stops applying is worthless, and refusing is the conservative direction. This
threshold bounds a session that is **already authenticated and doing the user's
work**. There the conservative direction is the other way round: the greater
harm is dropping legitimate work, not deferring a key refresh. The rule is not
"fail closed" or "fail open" but *fail towards whichever outcome is recoverable*,
and which one that is depends on who is on the other end.

**Against it, honestly:** a machine whose clock is permanently broken now runs
SSH sessions that never rekey on time and never say so. That is accepted because
the byte threshold — the one that actually bounds how much ciphertext a key
protects — needs no clock, so the property that matters most survives intact.

### How much in-flight data to hold during a rekey

Once we send `KEXINIT`, the client is still entitled to keep sending ordinary
channel data until *its* own `KEXINIT` goes out — roughly one network round trip
of the user's keystrokes and the shell's input. That traffic is neither part of
the exchange nor discardable, so it is buffered and dispatched afterwards. The
buffer is bounded at 64 packets, and a client that exceeds it is disconnected.

| Option | *What changes:* | Why not |
|---|---|---|
| **Bounded buffer, disconnect on overflow** (chosen) | A client that floods instead of answering gets "sent N packets instead of answering our KEXINIT" and is dropped. | A pathological but honest client on a very fast link could in principle be dropped. Sixty-four maximum-size packets in one round trip is not a shell session. |
| Unbounded buffer | Nothing, until a peer decides otherwise — at which point it is memory the peer chooses the size of. | A peer-controlled allocation in a daemon reachable before authentication is the shape of a denial of service. |
| Discard the deferred packets | The rekey always succeeds and never allocates. | Silently corrupts whatever the user was transferring while continuing to look like a working session — the worst available failure, because nothing reports it. |

**Revisit if** a legitimate high-throughput use (a large `scp`, a port forward
saturating a fast link) turns out to exceed 64 packets in a round trip in
practice. The fix would be to raise the bound or to count bytes rather than
packets, not to remove it.
