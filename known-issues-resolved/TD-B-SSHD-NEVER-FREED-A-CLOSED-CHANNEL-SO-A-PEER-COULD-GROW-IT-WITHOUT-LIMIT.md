## TD-B-SSHD-NEVER-FREED-A-CLOSED-CHANNEL-SO-A-PEER-COULD-GROW-IT-WITHOUT-LIMIT

**Status: FIXED**, 2026-09-05. A channel is now freed once both ends have
closed it; `MaxSessions` counts the whole table; the channel-number counter
refuses rather than overflows.

**In short:** an SSH connection carries several independent *channels* — one per
shell, per remote command, per file copy. The daemon created them and never
destroyed them. A channel the client had finished with stayed in the connection's
table until the client hung up, still holding whatever it had buffered — up to
two megabytes of the client's own keystrokes — with nothing left that could ever
read them, because the session's descriptors were dropped on the same pass. An
authenticated client that kept one channel open and cycled others could make the
daemon's memory grow for as long as it cared to, and the `MaxSessions` setting
did not stop it, because it counted only the channels still *running*.

### Where it lived

`userspace/sshd/src/lib.rs` — `Channel`, `handle_channel_open`,
`handle_channel_close`, `handle_channels`, `dispatch_channel_message`.

Three defects, one shape. Nothing owned the end of a channel's life:

```rust
// handle_channel_open: the limit counts only what is still running,
// so entries this side has closed are invisible to it.
let active = conn.channels.iter().filter(|ch| !ch.closed).count();
if active >= conn.config.max_sessions as usize { /* refuse */ }

let local_id = conn.next_channel_id;
conn.next_channel_id += 1;          // <-- unchecked, on a peer-driven counter
```

and no `retain`, anywhere, ever: `conn.channels` only grew.

### How to reach it

Authenticate, then repeat: open a session channel, send data to a program that
is not reading it (`ssh host 'sleep 1000' < /dev/zero` will do), close the
channel. Keep one channel open throughout so the connection does not end. Each
cycle leaves behind a `Channel` whose `pending_input` holds up to
`INITIAL_LOCAL_WINDOW` — 2 MiB — and nothing frees any of them.

A slower variant needs no data at all: open channels and simply never answer the
daemon's `CHANNEL_CLOSE`. Because half-closed entries did not count against
`MaxSessions`, the supply of slots was unlimited.

### Why it was invisible

`handle_channel_open` carried `#[expect(clippy::arithmetic_side_effects, reason
= "not yet audited for panics on peer-controlled input")]`. As with
`TD-B-SSHD-UNDERFLOWED-THE-LOGIN-GRACE-TIMER-WHEN-THE-CLOCK-FAILED`, the
suppression was not covering a false positive — it was covering the true one.
`conn.next_channel_id += 1` is an unchecked increment on a counter a peer
advances, which panics in a debug build and wraps in a release build, and a wrap
hands a new channel the number of a live one.

That is now the **second** of these suppressions to be opened and found to
contain a real, remotely-reachable defect. The remaining nineteen should be read
as unreviewed bug reports, not as settled exemptions.

The leak itself was invisible for a different reason: every individual piece was
correct. `handle_channel_close` carefully drops the terminal, kills the shell and
reaps the process — the expensive resources — and a reader checking for a leak
finds that code and stops. What it does not do is free the *entry*, and no test
asked whether the table ever shrank, because no test had a reason to look at a
table that only ever grew by one.

### The fix

Three changes, argued in `design-decisions.md` §1002:

- **`Channel::close_received` beside `Channel::closed`.** RFC 4254 §5.3 ends a
  channel when both ends have sent `CHANNEL_CLOSE`, and that is the earliest
  moment at which no message for it can still be in flight. The entry is
  reaped there. Two flags rather than one because the connection asks them
  different questions: *is there anything left to serve?* is answered by our own
  close alone — it must not wait on a peer that never replies — while *may this
  storage go?* needs both.
- **`MaxSessions` counts `conn.channels.len()`**, not the un-closed subset. A
  limit that excludes exactly the entries a hostile peer can accumulate is not a
  limit. A well-behaved client is unaffected: its close reaps the entry and
  returns the slot within a round trip.
- **`next_channel_id.checked_add(1)`**, refusing the open with
  `SSH_MSG_CHANNEL_OPEN_FAILURE` / resource-shortage when the numbers run out,
  reusing the reply `MaxSessions` already sends. Numbers are still issued once
  and never reused, so a channel number in a log names one session for the life
  of the connection.

`pending_input` is also emptied on close, which matters for the half-closed
window: an entry waiting on a peer that never answers now holds a `Vec::new()`
rather than 2 MiB.

Six tests pin it, including the two states that used to be indistinguishable —
a connection whose last channel was just reaped, and one that has not opened a
channel yet. Both are the empty table, and `all()` is vacuously true on an empty
table, so without `ConnectionState::channel_ever_opened` the connection loop
would hang up on its first pass.

### The general lesson

**A resource limit must count what the attacker can create, not what the happy
path leaves behind.** `MaxSessions` was written against the case it was named
for — how many shells may run at once — and the entries that accumulate are the
ones that are *not* running. The question to ask of any bound is not "does this
count the thing the feature is about?" but "what can a peer make that this does
not count?"
