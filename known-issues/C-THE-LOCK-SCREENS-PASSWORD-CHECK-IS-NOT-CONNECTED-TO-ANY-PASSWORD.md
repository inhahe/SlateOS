## C-THE-LOCK-SCREENS-PASSWORD-CHECK-IS-NOT-CONNECTED-TO-ANY-PASSWORD (lane C, 2026-08-18) — OPEN, cross-lane

**In short:** The desktop lock screen can check a password correctly, and the
system knows every user's real password, and there is no path between those two
facts. Worse than unconnected: the two sides store *different* scrambles of the
password, so they cannot be joined by plumbing. Filed to lane B as
`requests/c-b-the-lock-screen-has-no-way-to-check-a-real-password.md`.

### The two halves

`apps/lockscreen`'s `PasswordValidator` (line 270) verifies against a
`pwkdf::PasswordVerifier` — salt, rounds, 32-byte verifier — supplied by its
caller through `from_stored`. That is the right shape: the screen deliberately
does not own a store.

The store, though, is in lane B and speaks crypt(3): `/etc/shadow` holds
`$6$<salt>$<86 chars>` (SHA-512-crypt, `posix/src/crypt.rs`, §329), and
`userdb::Record::check_password` verifies against exactly that. There is no
lossless conversion from a `$6$` string to `(salt, rounds, verifier)`, and
there should not be — a hash you can convert is a hash you did not need.

So no production code constructs a `PasswordValidator` at all. The only
constructor with a caller is `#[cfg(test)] for_test`.

### Why it must not be fixed on this side

Two cheap fixes are available and both are wrong:

- **Teach the lock screen to read crypt strings.** That puts a second
  password-checking implementation in a GUI app — the exact defect that
  produced §329 (three tools, three disagreeing hashers) and §466 (two
  disagreeing KDFs). It also requires handing a password hash to an
  unprivileged process, which is what `/etc/shadow`'s permissions exist to
  prevent.
- **Give the lock screen its own password.** Then the screen unlocks with
  something that is not the account password and does not change when `passwd`
  runs — a machine that stays unlockable by an old password forever.

The correct shape is a privileged verifier: the screen sends the password to a
service that owns the store and gets back yes/no. Every other OS does this
(PAM). It is lane B's to build, which is why this is a request rather than a
patch.

### The related gap in `logind`

`userspace/logind/src/main.rs:882` — `unlock_session` sets `locked = false`
with no password, no caller check and no capability. As session bookkeeping
that is probably intended (systemd's `UnlockSession` is the same shape, with
the authentication in PAM); it is listed here only because it completes the
picture. Today the whole chain is: lock screen checks a password nobody
supplied → asks logind to unlock → logind unlocks unconditionally. The
authentication is decorative end to end.

### Not a duplicate of `init/loginmgr`

It looks like one and is not. A *login* screen authenticates someone who has no
session; a *lock* screen re-checks someone who already has one. Real systems
ship both. They should share the verification path and need not share anything
else. `roadmap.md` carries them as separate `[x]` items (lines 2614 and 6129)
with no cross-reference, which is what made this take a while to see.

### Independent of B-Q4

Which user store wins (`/etc/users.yaml` vs `/etc/passwd`+`/etc/shadow`) does
not change anything here: under the privileged-verifier shape the service reads
whichever store wins and this side is unchanged. That is a further argument for
that shape — it is the only one that does not have to wait for B-Q4.

**Discovered:** 2026-08-18, while auditing what actually calls the lock screen
after converting it onto `pwkdf`.
