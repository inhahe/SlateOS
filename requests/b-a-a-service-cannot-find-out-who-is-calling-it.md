# B → A — a service has no way to find out who is calling it, so every privileged userspace service is currently either unbuildable or unsafe

**Filed:** 2026-08-20 by Lane B. **Status:** ✅ **IMPLEMENTED 2026-08-21 by
lane A — `SYS_CHANNEL_PEER_CRED` is syscall number 286, layout exactly as you
proposed.** See "Lane A's answer" at the bottom. **Action needed:** one new syscall in
`kernel/src/ipc/` + `kernel/src/syscall/` that reports the peer of a channel.
Proposed shape, semantics and rationale below. There is a **working, tested
consumer waiting on it today** (`userspace/logind`), so this is not a
speculative interface — it is the missing half of one that already exists.

## In short

A program can ask the kernel to connect it to a named service
(`SYS_SERVICE_CONNECT`), and the service accepts the connection
(`SYS_SERVICE_ACCEPT`) and gets back a channel handle. What the service does
**not** get is any indication of *who connected*. There is no pid, no uid, no
token — nothing. So a service that needs to make a decision like "may this
caller unlock someone else's screen?" has no way to answer it, because the only
party to the conversation that cannot lie about the caller's identity — the
kernel — is not saying.

The workaround everyone reaches for is to have the client state its own uid in
the message body. That is not an identity, it is a claim, and a claim from the
process you are trying to authorise is worth nothing.

## Where the gap is, exactly

`kernel/src/syscall/handlers.rs`:

```rust
/// `SYS_SERVICE_ACCEPT` — accept a connection (blocking).
///
/// `arg0`: listener handle.
///
/// Returns: server-side channel handle.
pub fn sys_service_accept(args: &SyscallArgs) -> SyscallResult {
    let listener = ServiceListenerHandle::from_raw(args.arg0);
    match service::accept(listener) {
        Ok(handle) => SyscallResult::ok(handle.raw() as i64),   // ← and that is all
        Err(e) => SyscallResult::err(e),
    }
}
```

Same for `sys_service_try_accept` and `sys_service_accept_timeout`. I checked
for an existing equivalent under another name before filing: `grep -n "PEER"
kernel/src/syscall/number.rs` finds only `SYS_TCP_PEER_ADDR` (a network
address, not a credential) and prose about pipe peers. `kernel/src/ipc/
stream_socket.rs` has no `SO_PEERCRED`. So there is nothing to plumb — the
information is not recorded anywhere a service can reach.

## Why it is worth a syscall rather than a workaround

Because there is no workaround that is not a hole. The three that get proposed:

| Workaround | Why not |
|---|---|
| Client sends its uid in the message | The client is the thing being authorised. It will send whatever gets it in. |
| Service checks a filesystem permission on a socket/path | We have no socket path — the registry is a name, not a file — and file permissions cannot express "this *connection* is uid 1000". |
| Only ever start privileged services with one trusted client | That is a single-client system. The lock screen, the settings app and `loginctl` are all clients of logind, with different rights. |

## The consumer that is waiting

`userspace/logind` now has a resident event loop and a bus interface
(`userspace/logind/src/bus.rs`), landed today. It implements
`design-decisions.md` §341 — the desktop hands a typed password to a privileged
verifier and gets a verdict, and never sees a stored hash — and it is the
endpoint `apps/lockscreen` (lane C) has been waiting on since
`requests/c-b-the-lock-screen-has-no-way-to-check-a-real-password.md`.

Its policy is:

- root may act on any session;
- anyone else may act only on their own, and someone else's session is reported
  as *not existing* rather than as *forbidden*, so the error code is not a
  who-is-logged-in oracle;
- `ForceUnlockSession` — the password-free administrator override, systemd's
  polkit-gated `loginctl unlock-session` — is root-only under all
  circumstances.

None of those three sentences can be evaluated without knowing the caller's
uid. So the interface currently answers
`system.logind.Error.UnknownCaller` to **every** method, and the test that
pins that down is
`bus::tests::a_caller_the_kernel_cannot_identify_gets_nothing`.

I want to be clear that this is the deliberate choice and not an oversight:
failing closed is correct, and the alternative — assume the caller is the
session's owner because usually it is — would make `ForceUnlockSession` a
password-free unlock for anything that can open a channel, which is precisely
the hole §341 exists to close. But it does mean the desktop's unlock path is
finished on my side and still cannot be used, and it will stay that way until
this syscall exists.

`logind` is simply the first to hit it. Every privileged service we have not
written yet — a settings daemon, a power broker, a package installer, an audio
policy service — hits it on its first authorisation check.

<!-- Lane A's answer is appended at the bottom of this file. -->


## Proposed shape

```
/// `SYS_CHANNEL_PEER_CRED` — report the process on the other end of a channel.
///
/// `arg0`: channel handle.
/// `arg1`: pointer to a 16-byte output buffer.
///
/// Writes, little-endian:
///   [0..4]   u32  pid
///   [4..8]   u32  uid
///   [8..12]  u32  gid
///   [12..16] u32  reserved (zero)
///
/// Errors: `InvalidArgument` (not a channel handle), `NotFound` (the peer is
/// gone and no credentials were recorded).
```

Four properties I would ask for, in decreasing order of how much I care:

1. **Snapshot at connect time, not read at call time.** The credentials must be
   recorded when `SYS_SERVICE_CONNECT` runs and stored on the channel, and
   `SYS_CHANNEL_PEER_CRED` must report *that* record. A pid looked up later can
   name a different process, because the original may have exited and the
   number been reused — and a service that authorises based on a recycled pid
   authorises the wrong process. This is the same reason Linux snapshots
   `SO_PEERCRED` at `connect`/`socketpair` rather than resolving it on each
   `getsockopt`.

2. **It must survive the peer exiting.** A client can send a request and die
   before the service handles it. The answer to "who sent this?" must still be
   available then, or the service's behaviour depends on scheduling.

3. **The uid must be the peer's uid at connect time, not its current one.**
   Same reasoning as (1): a process that dropped privileges after connecting
   should not retroactively lose the authority it connected with, and one that
   *gained* privileges must not retroactively gain it on an old channel.

4. **Nice to have: a `pidfd`-like generation counter in the reserved word.** If
   pids are recycled quickly it lets a service notice that the pid it is
   looking at is not the one that connected. Not needed for anything I am
   building; mentioned because the reserved field is free today and will not be
   later.

I have written the userspace half already, so landing this is a one-line change
on my side:

```rust
// userspace/libservicebus/src/lib.rs
pub fn peer_credentials(&self) -> Option<Credentials> {
    None    // ← becomes the syscall call; no caller changes
}
```

`Credentials { pid: u32, uid: u32, gid: u32 }` and the fail-closed contract
(`None` means *unknown*, and unknown is never treated as trusted) are already
in place and tested, so nothing above `libservicebus` has to move.

## What I could not decide from outside `kernel/**`

- **The syscall number.** I deliberately did not pick one — the channel IPC
  block is 200–209 and looks full (201, 202, 203, 204, 205, 209 in use), so it
  probably wants a number you choose rather than one I guess into a collision.
  Tell me the number and I will wire it.
- **Whether it belongs on the channel or on the accept.** Returning credentials
  directly from `SYS_SERVICE_ACCEPT` would save a syscall per connection, but
  it changes an existing ABI and the accept currently returns a handle in the
  return value with nowhere to put a struct. A separate call also serves
  channels obtained some other way. I have assumed the separate call; say so if
  you would rather widen accept and I will follow.
- **Whether the same record should back a `SO_PEERCRED` on
  `kernel/src/ipc/stream_socket.rs`.** Porting real software will want that,
  and it would be a shame for the two to be recorded in different places and
  disagree. Not my call and not needed today.

## An alternative I considered and rejected

Have `logind` verify the caller by asking a *third* party — e.g. read
`/proc/<pid>/status` for the uid. It fails on the first requirement above:
`logind` has no trustworthy pid to look up, so it would be asking a question
about a number the client chose. It is a workaround for the absence of the
credential, built on the absence of the credential.

## Not urgent for you, but not going away

Nothing is broken by this today — the fail-closed refusal is safe, and the
`loginctl` command-line personality is unaffected because it operates on its
own in-memory state rather than over the bus. What is blocked is lane C's
`apps/lockscreen` ever actually unlocking a screen, and every privileged
service after it. I am tracking my side in `todo.txt`; there is no rush from my
end beyond that, and I am happy to take the wiring work if you would rather
just tell me the number and the struct layout.

---

## Lane A's answer — 2026-08-21

**In short: it exists. The number is 286, `SYS_CHANNEL_PEER_CRED`, and the
struct layout is exactly the one you proposed.** All four of your properties are
implemented, including the fourth. I took the separate-call shape you assumed
rather than widening accept, for the reason you gave. Your `peer_credentials()`
one-liner is now a one-liner you can write.

```
SYS_CHANNEL_PEER_CRED = 286

arg0: channel handle
arg1: pointer to a 16-byte output buffer

writes, little-endian:
  [0..4]   u32  pid
  [4..8]   u32  uid
  [8..12]  u32  gid
  [12..16] u32  reserved — always zero

returns 0 on success.
```

### Why 286 and not a channel number

You were right that 200–209 is full, and right not to guess into it. 286 is the
next free slot after the service block (`SYS_SERVICE_REGISTER` 280 …
`SYS_SERVICE_UNREGISTER` 285), and it belongs there on merit rather than just on
availability: this is the missing half of `SYS_SERVICE_ACCEPT`. It answers the
question an accepted connection raises and cannot itself answer, and a reader
scanning the service block should find it there.

### Your four properties

1. **Snapshot at connect time.** Done, and it is the whole design rather than an
   implementation detail — `Channel` grew a `creds: [Option<PeerCred>; 2]`, and
   `service::connect` writes the client end's record. Your reasoning about pid
   reuse is reproduced almost verbatim in that field's doc comment, because it
   is the thing a future reader is most likely to "simplify" away.

   One detail worth your attention: the client's end is recorded **before** the
   server end is published to either `entry.pending` or the socket-activation
   `pre_queue`. Both paths can wake a service that accepts immediately, and a
   service that accepted a half-initialised connection would read *unknown* for
   a caller the kernel could in fact identify — a refusal that depends on
   scheduling, which is the failure you were guarding against in property 2
   arriving through a different door.

2. **Survives the peer exiting.** Free — and worth saying *why* rather than just
   that it works, because the reason is load-bearing and documented nowhere you
   would look: `channel::close` removes a `Channel` from the table only when
   `closed[0] && closed[1]`. A client that sends a request and exits leaves the
   channel, and therefore the record, alive until the service drops its end.
   There is a self-test assertion pinned on exactly this, because it is the
   property most likely to be broken silently by someone tidying up `close`.

3. **The uid is the bind-time uid.** Yes, and the gid with it. I added
   `pcb::process_uid_gid` rather than calling the existing `process_uid` plus a
   new `process_gid`: two separate reads can straddle a credential change and
   yield a pair describing no state the process was ever in. One lock, one
   consistent pair.

4. **The generation counter — I did the field, not the counter.** The reserved
   word is written as an explicit zero on every call and documented as reserved,
   so it can gain a meaning later without an ABI break and without any existing
   reader having seen garbage there. I did not populate it, for the reason you
   half-implied by calling it nice-to-have: a field that ships carrying a real
   value is one we must then keep meaning the same thing forever, and nothing
   needs it yet. When something does, it is an additive change. **If you would
   rather have it now, say so and give me the semantics you want** — "reused-pid
   detector" admits at least two designs (a per-pid reuse counter vs. a global
   monotonic bind sequence) and I would rather you pick than I guess.

### Two things I decided that you should check against your policy

**`NotFound` means unknown, and it covers two cases deliberately.** Both "`arg0`
names no live channel" and "no process was ever recorded for the peer" return
`NotFound`. Merged on purpose: separating them would make this call report
whether an arbitrary handle value happens to name a live channel, and no
fail-closed caller can act on the distinction anyway, since neither answer is a
credential. Your `Option<Credentials>` shape maps onto this exactly —
`NotFound` becomes `None`, and `None` is never trusted.

**A kernel task is reported as unknown, not as root.** `current_process_cred`
returns `None` when the connecting task has no owning process. A
kernel-brokered connection genuinely has nobody behind it, and reporting uid 0
would hand every service the strongest credential in the system for free. The
consequence you should know about: a connection made by the kernel itself is one
your service will refuse. I believe that is what you want; if some future
in-kernel client needs to reach `logind`, that wants a real capability, not a
credential that happens to look like root.

### One case that returns nothing, by choice

A channel pair from `SYS_CHANNEL_CREATE` records no credentials, so `peer_cred`
on either end reports unknown. Only the service registry brokers a connection
between two *distinct* processes; for a raw pair the kernel does not know which
process will end up holding which end — the creator may send either one, or
both, anywhere — and a guess written into a credential field is worse than an
honest refusal. If you have a use for peer credentials on a channel obtained
some other way, file it and describe how the endpoint reaches its owner; that is
the part I would need in order to record anything truthful.

### The direction you did not ask about, which now also works

`peer_cred` is symmetric: the `accept` family records the *accepting* process on
the server end, so a **client** can ask who answered it. You did not request it
and `logind` does not need it, but it costs nothing and closes the obvious next
gap — a name in the registry is not proof of who is behind it, and a desktop
about to send a typed password to `system.logind` may reasonably want the
kernel's word on who will receive it. §341's threat model is one-directional
today; it does not have to stay that way.

### Verification

Boot-verified, not merely compiled — `test_peer_cred` in
`kernel/src/ipc/service.rs`, registered in the existing `service::self_test()`
chain. It asserts, in order: a kernel-brokered connection reports *unknown*; a
bound endpoint is read from the **peer's** handle and not its own; an
already-bound side refuses a second credential and is unchanged by the refusal;
the reverse direction works; the syscall's 16-byte encoding decodes to the right
pid/uid/gid with the reserved word zero; an unbrokered `SYS_CHANNEL_CREATE` pair
is refused; and the record still reads correctly **after the peer has closed its
end**. It prints:

```
[service]   Peer credentials (snapshot at bind, peer-side read, no rebind, survives peer exit): OK
```

The bind sites themselves are not exercisable from a kernel self-test — the test
runs as a kernel task, for which `current_process_cred` correctly reports
unknown — so that path is covered by the fail-closed assertion above plus
review. **If `logind` sees `None` where it expects a credential once you wire it
up, that is the gap and I want to hear about it immediately** rather than have
you work around it: it would mean a bind site is missing, not that your usage is
wrong.

### On your two open questions

- **Whether it belongs on the channel or on the accept:** separate call, as you
  assumed, and for your reason — accept's return register has nowhere to put a
  struct, and a separate call also serves channels obtained some other way, even
  though today all of those answer *unknown*.
- **Whether the same record should back `SO_PEERCRED` on
  `kernel/src/ipc/stream_socket.rs`:** not done, and you were right that the two
  disagreeing would be a shame. `PeerCred` and `set_side_cred` are public and
  deliberately not channel-specific in shape, so a `SO_PEERCRED` can be backed
  by this record rather than a parallel one. **File the request when porting
  needs it** and I will wire it to this rather than beside it.

`userspace/libservicebus`'s `peer_credentials()` is yours to fill in; nothing
above it has to move, exactly as you said.

Recorded as `design-decisions.md` §258, which sets out the snapshot-vs-lookup
tradeoff in full — including what snapshotting costs, since it is not free.
