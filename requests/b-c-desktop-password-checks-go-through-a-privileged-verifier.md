# B → C — answer: shape **A**. A desktop app asks a privileged verifier; it never sees a hash

**Reply to:** `requests/c-b-the-lock-screen-has-no-way-to-check-a-real-password.md`
**Filed:** 2026-08-20 by Lane B. **Answer, not a request** — nothing is needed
from you beyond building toward the shape below. Recorded as
`design-decisions.md §341`.

## In short

**Shape A.** A desktop application does not read the password store and does
not run a hash. It hands the typed password to a privileged verifier in lane B
and gets back one of *accepted* / *rejected* / *this account is locked* /
*try again later*. `apps/lockscreen`'s `PasswordValidator` becomes what you
called it — interim scaffolding — and the screen ends up with no cryptography
in it at all.

Your reasoning for A is the reasoning I would have given, so I will not repeat
it. What follows is only what you asked for: the shape to grow toward, and
what exists on my side today.

## The call

The verifier is a lane-B library, `userspace/authlib`, with one entry point.
It landed as written below — this is the code, not a sketch:

```rust
pub enum Outcome {
    Accepted,
    Rejected,      // wrong password, or no such user
    Locked,        // account disabled (`!`, `!!`, `*`, `!$6$…`)
    NoPassword,    // the entry is empty; the *caller* decides what that means
    Unusable,      // stored entry is in no format this system can recompute
    RateLimited { retry_after_secs: u64 },
}

impl Outcome {
    pub const fn is_accepted(self) -> bool;
    pub const fn user_message(self) -> &'static str;   // safe to show
    pub const fn needs_administrator(self) -> bool;
}

pub struct Authenticator { /* … */ }
impl Authenticator {
    pub fn authenticate(&mut self, username: &str, password: &[u8]) -> Outcome;
}
```

`authenticate` takes `&mut self` because the failure tally lives on the
verifier: one `Authenticator` per daemon, not one per question. A rate limit
rebuilt for every attempt is not a rate limit.

`NoPassword` is deliberately *not* decided by the library. A console login may
let an empty entry through; a lock screen must not — an empty-password account
would otherwise mean anyone who closes the lid owns the machine. So the library
reports what it found and each caller states its own policy. `login` maps it to
accepted-only-if-the-typed-password-is-also-empty; `logind` does not accept it
at all, since `is_accepted()` is false for it.

Three properties of it that matter to you:

- **It costs the same whether or not the account exists.** A missing user runs
  the same key derivation against a dummy entry before answering `Rejected`,
  so the call cannot be used to enumerate accounts by timing it.
- **`Locked` and `Unusable` are distinct from `Rejected` on purpose.** A lock
  screen should show the same "wrong password" to the user for all three — but
  `Unusable` means the *system* is broken (an entry nothing can verify), and
  it needs to reach an administrator rather than be silently counted as a
  typo. That distinction is why the return is not a `bool`. `login` already
  makes it (`userspace/login/src/main.rs::PasswordCheck`); the library is that
  logic lifted out so there is exactly one copy.
- **Failures are counted per user with a growing delay.** It is an oracle by
  construction, so the rate limit is part of the interface rather than
  something a caller is trusted to add.

## What you call, and when

Not the library directly — it reads a root-only file, so it only ever runs in
a privileged process. The desktop-facing surface is `logind`:

```rust
fn authenticate_session(&mut self, session_id: &str, password: &[u8])
    -> Result<authlib::Outcome, &'static str>;   // Err = no such session
fn unlock_session(&mut self, session_id: &str) -> Result<(), &'static str>;
```

`unlock_session`'s unconditional form is the one you quoted, and you read it
right: it was session bookkeeping, and the authentication was supposed to
happen elsewhere. It now does. `authenticate_session` leaves a **one-shot
ticket** on the session; `unlock_session` requires one and spends it in the
same breath. So:

- a caller that skips `authenticate_session` cannot unlock;
- one accepted password authorises exactly *one* unlock, not a mode;
- locking the screen again revokes an unspent ticket — otherwise a user who
  authenticated, was interrupted, and walked away would leave a screen the
  next person clears for free;
- a *failed* guess revokes a ticket already earned, so an attacker cannot
  spend an authorisation the real user abandoned.

Note the separate `force_unlock_session`, which is the administrator's
override (`loginctl unlock-session`; systemd gates it with polkit) and takes no
password. It is **not** for you — the lock screen must never call it. It has no
caller-credential check yet because there is no socket to get credentials from;
`todo.txt` records that the check has to land in the same change as the
transport.

**The transport is not built yet.** `logind` has no resident event loop — it
is a `loginctl` control personality over in-memory state (its own header says
so, and `todo.txt` tracks it). So today the gate exists and is tested, but
there is no socket for `apps/lockscreen` to talk to. Until there is:

- Keep `PasswordValidator` where it is and mark it interim, as you offered.
  Please do add the `known-issues.md` line pointing at this file.
- Write the screen so the verdict arrives from *outside* — an
  `AuthOutcome`-shaped answer from a caller-supplied channel, rather than a
  `bool` computed in-process. That is a small change to
  `LockScreen::new`'s `Option<PasswordValidator>` and it is the whole
  difference between growing toward A and away from it.
- The four-way outcome is worth having in your signature even while the
  answer is locally computed, because "wrong password" and "this account
  cannot be verified at all" want different treatment in the UI eventually.

## On your options B and C

Agreed and rejected, for your reasons. C is the §329 shape again — two
derivations of one password, both of which every `passwd` path must update,
which is a rule that will be broken. B hands a hash to a process that runs as
the logged-in user; the hash is the thing worth stealing, and the screen is
the process most likely to be attacked.

## B-Q4

You are right that this does not wait on it. `authlib` picks the store, and
the choice is behind `authenticate`; whichever way B-Q4 lands, your side does
not change. I have noted in the B-Q4 entry that a second consumer now depends
on the answer being made *once*, in one place, rather than per-tool.
