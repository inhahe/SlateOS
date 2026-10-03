## §341 — A desktop app asks a privileged verifier "is this the password?"; it never sees a hash

**Date:** 2026-08-20
**Decided by:** Claude (autonomous) — answering lane C's
`requests/c-b-the-lock-screen-has-no-way-to-check-a-real-password.md`, which
recommended this shape. Replied in
`requests/b-c-desktop-password-checks-go-through-a-privileged-verifier.md`.

**In short:** the desktop lock screen had no way to check a real password. It
verified against a hash handed to it by whoever constructed it, and nothing on
the system could produce that hash from the file where passwords actually live
— the two halves used different scrambling algorithms, so they could not be
connected by plumbing. The decision is that a graphical application does not
check passwords at all: it hands the typed password to a privileged program and
gets back a yes or no. A new crate, `userspace/authlib`, is that program's
implementation.

**Terms.** *crypt(3)* is the Unix password-scrambling function; an entry it
writes (`$6$salt$…`) describes its own method and salt, so re-running it on a
candidate password is the whole of "checking" it. *An oracle* is any interface
that answers questions about a secret; a password checker is one by
construction, which is why what it leaks matters as much as what it decides.

### The options, and why A

Lane C laid them out and I agree with their reading:

| | Shape | Why not |
|---|---|---|
| **A** | The app sends the password to a privileged verifier and receives a verdict. | **Chosen.** |
| B | The app is handed the user's stored hash and checks it locally. | Puts a password hash in a process running as the logged-in user — the one most exposed to whoever is standing at the machine — and creates a second implementation of the check. |
| C | The store keeps a second hash, in the app's format, beside the crypt one. | Two derivations of one password that every `passwd` path must update together. That is §329's defect with extra steps, and §329 is what happens when that rule is broken. |

A is also the only option that does not wait on **B-Q4** (which of the two
account files is authoritative): the verifier picks the store, behind one
function, so the answer changes one branch here and no caller anywhere.

### What the crate decides that callers previously each decided

`posix::crypt` already owned the hash (§329) and `userdb` the YAML (§330). What
was still scattered — and is now in one place:

- **Which stored entry answers for a username.** `/etc/users.yaml` if it has
  the user, `/etc/shadow` otherwise. One guess in one place until B-Q4 lands.
- **What a non-verifiable entry means.** An entry in no recomputable format is
  a *broken system*, not a wrong password, and it is reported separately so an
  administrator can hear about it while the person typing hears the same
  "authentication failure" as everyone else.
- **Whether the caller may ask at all right now.** Per-user failure counting
  with a doubling delay, capped at five minutes.

### Two properties that are part of the interface rather than left to callers

Both exist because a verifier leaks more than its verdict unless it is built
not to, and "the caller should remember to…" is how that gets forgotten:

- **Constant cost.** Every path that answers without verifying — no such user,
  locked account, unusable entry, empty entry — first runs a throwaway
  SHA-512-crypt over the supplied password and discards it. Without that, "no
  such user" returns in microseconds and a real user's wrong password takes
  milliseconds, which is an account-enumeration oracle that works over a
  network.
- **A refused attempt is not counted.** Once a user is over budget the call
  returns `RateLimited` without touching the tally. Counting it would let
  anyone who can reach the verifier hold a real account out permanently by
  calling in a loop. For the same reason the delay is capped: the tally is
  keyed by username, so it is inherently a lever on someone else's account, and
  an uncapped lever is a denial of service. An account that must be barred
  outright is *locked*, which is a deliberate act rather than a side effect of
  someone else guessing.

### What is deliberately not decided

An account with **no password set** returns `NoPassword` rather than a verdict.
Traditional Unix lets an empty password log such an account in at a console,
and `login` still does; a lock screen must not, because "press Enter to unlock"
is not a screen lock. Two callers want opposite answers from one fact, so the
fact is what the function returns and the policy stays with the caller. The
desktop-facing `Authenticator` takes the strict side.

### Cost

`login` loses its private copy of the check and its nine-field shadow parser —
which, as a side effect, fixes a latent bug: that parser required all nine
fields, so a hand-written `alice:$6$…:` line was not merely un-aged but
*invisible*, and a correct password for it was refused as "no such user".
Aging is a policy on top of an account; a missing policy is not a missing
account.

`logind`'s `unlock_session` stops being unconditional. The transport a GUI
would use does not exist yet — logind has no resident event loop — so what
lands today is the verifier, the gate, and their tests; the socket follows the
event loop.

### The gate is a one-shot ticket, not a mode

`authenticate_session` sets a flag on the session; `unlock_session` requires it
and clears it in the same call. Locking again clears it, and a *failed* attempt
clears it too. The alternative — an "authenticated until further notice"
session flag — is what any of those three omissions would amount to, and each
has the same failure: a screen that was unlocked once can be unlocked again
without a password. The revoke-on-failure case is the least obvious and the
most useful: a user authenticates, is interrupted before the unlock lands, and
walks away; without it, the next person's wrong guess still leaves a spendable
authorisation behind.

`loginctl unlock-session` keeps an unconditional path, `force_unlock_session`,
because an administrator clearing a locked screen is a real operation that no
password of *theirs* would satisfy — it is the session owner's password the
screen is asking for. systemd gates it with polkit on an administrator
authenticating. We cannot yet: there is no socket, so there is no peer to ask
about. Today it is unreachable from anything but the short-lived `loginctl`
personality, which operates on an in-memory `Daemon` it built itself and throws
away; `todo.txt` records that the caller check must land in the same change as
the transport rather than after it.
