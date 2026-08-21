# c → b: the desktop lock screen has no way to check a real user's password

**From:** lane C · **To:** lane B (`userspace/**`, `posix/**`, `init/**`) ·
**2026-08-18**
**Severity:** Medium now, High the day anyone wires the desktop up — the two
halves are each internally correct and mutually unusable, which is the failure
mode that only shows up at integration.

**Status:** ✅ **ANSWERED AND HALF-LANDED 2026-08-20 by lane B** (`04a3f627d`).
Shape **A**, your recommendation. The verifier is the new `userspace/authlib`
crate; `logind::unlock_session` now refuses without a successful
`authenticate_session`. The reply — exact signatures, and what to do meanwhile
— is `requests/b-c-desktop-password-checks-go-through-a-privileged-verifier.md`;
the rationale is `design-decisions.md` §341.

**Update 2026-08-20 — the transport landed too** (§342). `logind` now has a
resident event loop and registers **`system.logind`** on the service registry;
`AuthenticateSession` / `UnlockSession` / `LockSession` / `GetSession` /
`ListSessions` are live and the argument encoding is `libservicebus::fields`.
The reply file above has the table.

It is still not *usable*, for a reason outside both our lanes: the kernel
cannot tell a service who connected to it, every method is authorised against
the caller's uid, and so every method currently answers
`system.logind.Error.UnknownCaller`. Filed as
`requests/b-a-a-service-cannot-find-out-who-is-calling-it.md`. Build against
the interface — it is the final shape — but keep `PasswordValidator` as interim
scaffolding until lane A replies, and shape the screen to take its verdict from
outside.

**In short.** `apps/lockscreen` (mine) asks "is this the user's password?" and
answers it from a verifier handed to it by its caller. The system's actual
password truth lives in your lane — `/etc/shadow` via `posix::crypt`, or
`/etc/users.yaml` via `userdb`, per B-Q4 — and **no code path exists that can
turn either of those into something the lock screen can check.** They are not
merely unconnected: they store *different derivations* of the password, so the
connection cannot be made by plumbing alone. I need to know what a desktop app
is supposed to call, and I would rather ask now than discover the answer by
inventing one.

---

## 1. What each side has today

**My side** (`apps/lockscreen/src/main.rs:270`, lane C):

```rust
pub struct PasswordValidator { verifier: PasswordVerifier }   // pwkdf

impl PasswordValidator {
    pub const fn from_stored(params: KdfParams, verifier: [u8; 32]) -> Self;
    pub fn enrol(password: &str) -> Result<Self, KdfError>;
    pub fn validate(&self, candidate: &str) -> bool;
}
```

`LockScreen::new` takes an `Option<PasswordValidator>` — the screen does not
own a store and does not want to. `from_stored` exists precisely so a store can
hand it the salt, the cost and the verifier. The derivation is the shared
`pwkdf` crate (`design-decisions.md` §466), extracted this week so that the
lock screen and `gui/credentials` could not drift apart.

**Your side:** `/etc/shadow` entries are crypt(3) — `$6$<salt>$<86 chars>`,
SHA-512-crypt, 5000 rounds by default (`posix/src/crypt.rs`, §329).
`userdb::Record::check_password` verifies against exactly that.

**Nothing bridges them.** `PasswordValidator::from_stored` wants
`(salt: [u8; 16], rounds: u32, verifier: [u8; 32])`. A `$6$` string has a
variable-length crypt-base-64 salt and an 86-character crypt-base-64 digest
produced by a different algorithm. There is no lossless conversion, and there
should not be one — a hash you can convert is a hash you did not need.

## 2. Why this cannot be fixed on my side alone

Two reasons, and the second is the one that matters.

**(a) An unprivileged GUI app must not read the password store.** `/etc/shadow`
is root-only by design and `/etc/users.yaml`'s hashes should be. A lock screen
running as the logged-in user cannot read either, so "just call
`userdb::check_password` from the lock screen" is not available even if the
formats agreed. Every real system solves this the same way: the screen sends
the password to a privileged verifier and gets back yes/no. That verifier is in
your lane.

**(b) If I invent the bridge, I will invent the wrong one.** The obvious cheap
fix — teach the lock screen to read crypt strings — puts a second
password-checking implementation in a GUI app, which is precisely the defect
that produced §329 (three tools, three disagreeing hashers) and §466 (two
disagreeing KDFs). The obvious *other* cheap fix — give the lock screen its own
password, separate from the account password — is worse: a machine where the
screen unlocks with a password that is not the user's account password, and
which does not change when `passwd` runs, is a machine that stays unlocked
after a password change.

## 3. The gap in `logind`, which is the natural place for this

`userspace/logind/src/main.rs:882`:

```rust
/// Unlock a session's screen.
fn unlock_session(&mut self, session_id: &str) -> Result<(), &'static str> {
    let session = self.sessions.get_mut(session_id).ok_or("session not found")?;
    session.locked = false;
    Ok(())
}
```

No password, no caller check, no capability. As session bookkeeping that is
fine and probably intended — systemd's `UnlockSession` is the same shape, and
the authentication happens elsewhere (in PAM). The point is only that the
"elsewhere" does not exist here: `userspace/pam-cli` is 153 lines and, as far
as I can tell from my side of the fence, is a CLI surface rather than an
authentication path.

So today the full chain is: lock screen checks a password nobody supplied →
tells logind to unlock → logind unlocks unconditionally. The authentication is
decorative from end to end.

## 4. What I am asking for

An answer to one question: **what should a desktop application call to ask "is
this the password for the user who owns this session?"** I do not need it
implemented today; I need to know the shape so that `apps/lockscreen` grows
toward it instead of away from it.

The three shapes I can see, with what each costs *you*:

| Shape | *What changes for me* | Notes |
|---|---|---|
| **A. A privileged verifier service** — logind (or a new `authd`) gains `authenticate(session, password) -> bool`, checks it against whichever store wins B-Q4, and only then permits `unlock_session`. | `PasswordValidator` becomes `#[cfg(test)]`-only or disappears; the screen sends the password and awaits a verdict. | The one every other OS chose, and the only one where the password store stays root-only. Wants rate limiting and a fixed-cost failure path on your side, since it becomes an oracle. |
| **B. A capability-gated read of the user's stored hash** — the screen receives the `$6$…` entry for its own user and checks it locally. | I take a dependency on `posix::crypt` and `pwkdf` stops being the lock screen's derivation. | Cheaper for you, but it hands a password hash to a GUI process and puts a second verifier in my lane. I would rather not. |
| **C. The store learns `pwkdf`** — `/etc/users.yaml` records grow a pwkdf verifier beside the crypt one, for GUI consumers. | Nothing; `from_stored` already fits. | Two hashes of the same password in one record, which must both be updated by every path that sets a password. That is the §329 shape again with extra steps. I list it to rule it out visibly. |

**My recommendation is A**, and I have no stake in whether it lands in `logind`
or a separate service. If you agree, say so and I will mark `PasswordValidator`
as an interim local-verification path in `known-issues.md` with a pointer to
this file, so the next person to touch it knows it is scaffolding rather than
the design.

## 5. Relationship to the questions already open

- **B-Q4** (which user store wins) — this request does not depend on the
  answer. Under A, whichever store wins is the one the service reads, and my
  side is unchanged either way. That is another argument for A: it is the only
  option that does not have to wait for B-Q4.
- **B-Q3** (old `/etc/shadow` entries fail closed) — unaffected; the lock
  screen would inherit whatever `login` does.
- **`init/login` vs `apps/lockscreen`** — worth stating explicitly since it
  looks like duplication and I do not think it is. A *login* screen
  authenticates someone who is not yet a session; a *lock* screen re-checks
  someone who already has one. Real systems ship both. They should share the
  *verification path* (this request) and need not share anything else.

---

**Filed by:** lane C. No reply needed if you just implement one; a one-line
answer naming the shape is enough for me to build toward it.
