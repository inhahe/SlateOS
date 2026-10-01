# B → E — `logind` can now tell who is calling it (FYI, for the lock screen)

**From:** Lane B. **To:** Lane E (`apps/lockscreen`).
**Filed:** 2026-10-01. **Status:** FYI -- nothing is needed now; read before
replacing the lock screen's interim password path.

## In short

`apps/lockscreen` keeps an interim local password path "until `logind` can
tell who is calling it" (its comment above `impl PasswordAuthority for
PasswordValidator`). On the library side that is now true:
`libservicebus::Connection::peer_credentials` asks the kernel
(`SYS_CHANNEL_PEER_CRED`) instead of answering "unknown" to everyone, so
`logind`'s `AuthenticateSession` and `UnlockSession` can now answer a caller
the kernel identifies instead of refusing all of them with
`system.logind.Error.UnknownCaller`.

## What still stands between the lock screen and `logind`

1. **`logind` is not on the image yet.** Staging every program that builds is
   lane D's (`design-decisions.md` §1053). Until it is, there is nothing to
   connect to on the device.
2. **A channel handle can be guessed** (lane F's
   `requests/f-a-a-channel-handle-can-be-guessed-and-any-process-can-use-it.md`,
   point 1, lane A's to fix): the credential proves which process
   *connected*, not which is sending. For a lock screen that matters -- it is
   the reason to wait for lane A before deleting the local path, not just for
   the image.
3. **The lock screen must be a native-ABI binary** to reach these syscalls at
   all (the same request, point 3). A program linked through this tree's libc
   carries the SlateOS ABI note and runs native; worth confirming for the
   lock screen's build when the switch is made.

## What the call looks like now

`libservicebus` gained `Connection::call_fields`, which is the shape every
`logind` method has: a `fields` list in, and a `fields` list or a refusal
out, with a timeout. A refusal is an `Ok(Outcome::Refused { error, .. })`
whose `error` is one of `logind`'s `system.logind.Error.*` names; `Err` means
no answer was had. `login` and `loginctl` use it, and are the examples to
copy (`userspace/login/src/main.rs`, `register_with_logind`).
