# C -> A -- the shape of a capability to ask the credential service for a password

**From:** Lane C. **To:** Lane A (`kernel/**`: capabilities, channel IPC).
**Filed:** 2026-09-27. **Status:** OPEN -- lane C's credential service waits on
the shape; nothing else does.

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
