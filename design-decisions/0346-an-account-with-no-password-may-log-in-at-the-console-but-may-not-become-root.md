## §346 — An account with no password may log in at the console but may not become root

**Date:** 2026-08-21
**Decided by:** Claude (autonomous)

**In short:** Some accounts have no password at all — the password field in
`/etc/shadow` is simply empty. Two programs have to decide what that means.
`login` (the console login prompt) treats it as "press Enter and you're in".
`doas` (the program that runs one command as root after you type your password)
treats it as a refusal. Those are opposite answers to the same stored fact, and
this records why that is deliberate rather than an inconsistency to be tidied
away.

`authlib` — the one place SlateOS answers "is this the user's password?" —
deliberately does not resolve this. It returns a distinct
`Outcome::NoPassword` meaning *nothing was verified, and here is why*, and
leaves the policy to the caller. That was already the design (§341); this
section is the second caller arriving and confirming the split was needed.

**The two answers**

| Caller | `NoPassword` means | Reasoning |
|---|---|---|
| `login`, at the machine's own keyboard | Accepted, if the typed password was also empty | A deliberately passwordless account is a long-standing Unix choice — a kiosk, a single-user machine, a recovery console. The user is already standing at the hardware; a password adds nothing they have not already got |
| `doas`, escalating to root | Refused | "This account needs no password to *be* itself" is not the same statement as "this account needs no password to become *root*". Reading the first as the second turns every passwordless account into a root shell |
| A desktop lock screen | Refused | Already the position `authlib`'s docs cite. "Press Enter to unlock" is not a screen lock |

**The alternative, and why not**

The tidier-looking option is for `authlib` to fold `NoPassword` into either
`Accepted` or `Rejected` so that every caller behaves the same. Both foldings
are wrong somewhere:

- **Fold into `Accepted`** and the lock screen and `doas` both open to anyone
  who presses Enter. The lock screen case is the one that makes this
  indefensible: the whole point of a lock screen is the person at the keyboard
  is *not* trusted.
- **Fold into `Rejected`** and a passwordless console account can no longer log
  in at all, which breaks the legitimate configuration and — worse — reports it
  as a wrong password, so the administrator's diagnosis is "I must have set a
  password and forgotten it".

The cost of the split is that the policy lives in three places and could drift
between them. That is mitigated by making it a *named* outcome rather than an
absence: a new caller that writes `if outcome.is_accepted()` gets the safe
answer without having thought about it, and has to go out of its way to get the
permissive one. `doas` needed no special code at all — the refusal is what
`is_accepted()` already gives.

**Where it bites:** `userspace/login/src/main.rs` `check_password` (maps
`NoPassword` + empty input to `Accepted`); `userspace/doas/src/main.rs`, the
authentication block in `main` (gates on `is_accepted()`, so no mapping). If a
third policy is ever needed, the place to put it is a named helper in `authlib`
— `authlib::console_login_policy(outcome, typed)` — rather than a fourth
open-coded `match`.

**Related:** §341 (`authlib` exists at all), §329 (the three disagreeing
hashers that led to it), `known-issues.md` →
`B-DOAS-COULD-NOT-VERIFY-ANY-PASSWORD-THE-SYSTEM-ACTUALLY-SETS`.
