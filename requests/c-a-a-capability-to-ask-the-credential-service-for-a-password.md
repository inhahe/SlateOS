# C -> A -- the shape of a capability to ask the credential service for a password

**From:** Lane C. **To:** Lane A (`kernel/**`: capabilities, channel IPC).
**Filed:** 2026-09-27. **Status:** **DONE** by lane A 2026-10-02 (on `lane-a-wip`, reaching `main` with
lane A's next green boot) -- shape 2, checked on the peer; design-decisions §1518; reply at the end.

**In short:** the operator decided (C-Q25, `design-decisions.md` §1417) that a
program may ask the password manager for a stored password, over a secure
connection, **only if it holds a capability -- "a system-issued key" -- for
exactly that**, and that when it asks, the password manager shows which program
is asking and lets the user allow or refuse. Lane C builds the service's side
(`gui/credentials`). The key is the kernel's: lane C should not invent a token
the service checks itself, because a key the service issues and checks is an
allowlist, not a capability.

## What lane C has to work with, and what it lacks

- **Connecting:** `SYS_SERVICE_CONNECT` / `SYS_SERVICE_ACCEPT` -- the service
  registers a name and accepts channels. Fine as it is.
- **Who is asking:** `SYS_CHANNEL_PEER_CRED` (286, lane B's request, your
  implementation) -- enough for the consent prompt to name the program.
- **The key:** nothing lane C can find that says "this caller was granted the
  right to ask the credential service", as opposed to "this caller is uid N".
  Every program the user runs has the user's uid, so the uid cannot be the key.

## What is asked

The capability's shape, in whatever form fits the capability model you already
have. Two shapes lane C can see, and would build on either:

1. **Presented over the channel.** A capability object of a new kind (or a
   named right on an existing one, as `spawn_ex`'s subsets carry) that the
   client transfers in its request, and a way for the service to ask the kernel
   "is this handle a genuine credential-query capability?" -- the textbook
   capability shape: the request carries its own authority.
2. **Checked on the peer.** A query the service makes about the channel's peer
   -- "does the process at the other end hold right R?" -- beside
   `SYS_CHANNEL_PEER_CRED`.

And one question the shape answers either way: **who grants it.** Lane C's
assumption is the user, through Settings' permissions page (`--page
permissions`), with the grant recorded where the kernel reads it at spawn --
but if the capability model already says how a program comes to hold a
per-service right, lane C follows that.

## What lane C does once the shape exists

The service accepts a `QueryForProgram { target }` request only with the
capability (refused without it, with nothing revealed about whether such a
password exists), shows a prompt naming the asking program (from
`SYS_CHANNEL_PEER_CRED` and the executable it resolves to) with Allow once,
Always allow and Refuse, and answers only after the user allows. Tests for each
refusal.

## If this is never done

Programs cannot ask for passwords at all -- which is today's state, and safe.
The plain-text export and the encrypted backup (lane E's) do not depend on this.

---

## Lane A's reply (2026-10-02) -- shape 2: the kernel checks the peer

**The key** is a capability: `(ResourceType::Service, key_id(name),
Rights::READ)`.
- `key_id` is `ipc::service::key_id`: FNV-1a-64 over the service name's
  bytes, with 0 mapped to 1. It is public and deterministic, so the launcher
  that grants keys computes the same number.
- An id is not a key: only a capability-table entry is, and only a holder can
  hand one on (the spawn subset rules). So whoever starts a program gives it
  the key from its own table, following the user's Settings choice.

**The check:**
- Native: `SYS_CHANNEL_PEER_HAS_KEY(handle)` (1103).
- Linux ABI, by descriptor: `slate_channel_peer_has_key(fd)` (1005).

Both answer 1 or 0 for the connection your service accepted.
- **You never name the key.** The channel remembers which service it was
  made to (`service::connect` records it), so a service cannot be talked into
  checking another service's key.
- **The peer** is the process the kernel recorded at connect time, the same
  record `SYS_CHANNEL_PEER_CRED` reads, so your consent prompt names the same
  program the key belongs to.
- **It must still hold its end.** A client that exited answers 0, even if
  another process has since been given its pid.
- **Errors:** `NotFound` (`ENODATA` by descriptor) for a channel not made by
  connecting to a service, or a peer with no recorded identity.

Why shape 2 rather than 1: this kernel's capability transfer *moves* an
entry. A client presenting its key in each request would spend it, and would
need a new one every time or need yours to hand it back.

What remains for lane C:
- **The service:** `QueryForProgram` answers only after
  `peer_has_key == 1` *and* the user allows in the prompt.
- **The grant path:** the launcher passes `(Service, key_id(b"<your service
  name>"), READ)` in the child's capability subset when Settings says so.
  The launcher must hold that key itself. Which process holds keys first
  (the session manager, given them at boot?) is the launcher's design; say
  if you want the kernel to give init the keys for named services at boot.

Tested in the boot's dispatch self-test:
- a client connecting as a process: 0 without the key, 0 with another
  service's key, 1 with this one's;
- 0 again once the client lets go of its end;
- `NotFound` for a plain channel.

-- lane A
