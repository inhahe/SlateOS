## 1002. An SSH channel is freed when both ends have closed it, and its number is never handed out twice

**Date:** 2026-09-05
**Lane:** B
**Decided by:** Claude (autonomous)

**In short:** an SSH connection carries several independent "channels" — one per
shell, per remote command, per file transfer — and each has a number the two
ends use to say which one they mean. Ours were created and never destroyed: a
channel the client finished with stayed in the daemon's table, holding up to two
megabytes of buffered keystrokes, until the whole connection dropped. A client
that kept one channel open and cycled others could make the daemon's memory grow
without limit, and the `MaxSessions` setting did not stop it because that setting
counted only the channels still *running*. The decisions here are when to free a
channel, what `MaxSessions` should count, and whether a freed number may be
issued again.

**Jargon, once.** *Channel*: one stream inside an SSH connection; a session with
two shells open has two. *Channel number*: the small integer each end uses to
address a channel; the two ends number them independently. *Close*: each end
sends a `CHANNEL_CLOSE` message once, and the channel is over when both have
been sent — so there is a window in which one end has closed and the other has
not. *Half-closed*: a channel in that window.

### Decision 1: free the channel on the second close, not the first, and not at hang-up

| Option | *What changes:* |
|---|---|
| Free at hang-up (what it did) | Memory a client finished with is held until it disconnects; a long-lived connection grows without bound. |
| Free when *we* close | The number could be reissued while the client still believes it owns it, so a message still in flight for the old channel lands on a new one. |
| **Free when both have closed (chosen)** | Memory is returned one round trip after the client is done, and no message can arrive for a channel that no longer exists. |

The second close is what the protocol itself names as the point a channel number
becomes free (RFC 4254 §5.3), and it is the earliest point at which no message
for that channel can still be in flight. Freeing earlier is a correctness bug,
not merely an aggressive optimisation.

**Against it, honestly:** it requires tracking the two closes separately rather
than one "closed" flag, and the two flags are asked different questions in
different places, which is a thing a reader has to hold. The alternative — one
flag — cannot express "we are done but the peer has not said so", which is
precisely the state that has to be represented.

### Decision 2: `MaxSessions` counts every channel in the table, including half-closed ones

| Option | *What changes:* |
|---|---|
| Count only running channels (what it did) | A client that never answers a close can open sessions without end; the limit bounds concurrency but not memory. |
| **Count the whole table (chosen)** | The limit bounds memory too. A client cycling sessions very fast may briefly be told "max sessions exceeded" while a close is in flight. |

The limit exists to bound what one connection can make the daemon hold. A count
that excludes exactly the entries a hostile peer can accumulate does not do that.

**Against it, honestly:** a well-behaved client that opens and closes sessions
back-to-back now has each slot occupied for one extra round trip, so at the
default of ten sessions a client churning faster than the link's latency can see
a refusal it would not have seen before. That refusal is retryable and names its
reason, and the alternative is a limit that a peer can ignore entirely.

### Decision 3: channel numbers are issued once and never reused

| Option | *What changes:* |
|---|---|
| Reuse freed numbers | The supply never runs out. A channel number in a log names several different sessions, and a straggling message for a closed channel is delivered to a live one. |
| **Issue once, refuse when exhausted (chosen)** | A number in a log names one session for the life of the connection. After four billion channels on a single connection, further opens are refused with "channel numbers exhausted". |

The old code did neither: it incremented an unchecked counter, which panics in a
debug build and wraps in a release build — and a wrap is the reuse option with
none of its care, since it would hand a new channel the number of a live one.

**Against it, honestly:** the exhaustion path is unreachable in practice, and
code that cannot be reached in production is code that is only ever exercised by
its test. That is the reason it is a plain refusal reusing the same reply the
`MaxSessions` limit already sends, rather than anything with its own machinery:
the unreachable branch should be the *least* novel code in the function.

**If it is never revisited:** nothing degrades. All three are strictly safer than
what they replaced.
