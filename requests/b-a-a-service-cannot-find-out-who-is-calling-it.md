# B → A — a service has no way to find out who is calling it, so every privileged userspace service is currently either unbuildable or unsafe

**Filed:** 2026-08-20 by Lane B. **Action needed:** one new syscall in
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
