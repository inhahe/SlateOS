## §329 — The three shadow-file tools call libc's `crypt`, and entries in the two invented formats fail closed

**Date:** 2026-08-17
**Decided by:** Claude (autonomous)

**In short:** Three programs — `login`, `passwd` and `chpasswd` — all read and
write the one file that stores users' passwords, `/etc/shadow`, and each of
them scrambled the password a different way before storing it. The result was
that a password you set with `passwd` could not be used to log in: `login`
scrambled the typed password by a different rule and concluded it was wrong.
Two things had to be decided. First, *where* the one true scrambling rule
should live — in a library the three tools share, and if so which one. Second,
what to do about passwords already stored in the wrong formats: refuse them and
make an administrator reset them, or keep the old rules alive so those users
can still log in. We made the three tools call the copy already in the `posix`
crate, and we made old entries fail closed with an explicit message naming the
fix.

**Terms.** *Hashing* a password means running it through a one-way scramble so
the file stores something an attacker cannot reverse into the password.
`crypt(3)` is the C library function every Unix uses for this; its output is
self-describing — a stored entry like `$6$salt$hash…` names its own method
(`$6$` = SHA-512) and carries its own *salt* (a per-user random string that
stops two users with the same password having the same entry). *Failing
closed* means that when the system cannot tell whether a password is right, it
answers "no" rather than "yes".

### What was actually wrong

Lane C found it and filed
`requests/c-b-passwd-and-login-disagree-about-etc-shadow.md`. Verified against
the code before anything was changed:

| Tool | Wrote / read | Was it a hash? |
|---|---|---|
| `passwd` | `$sha256$salt$…` | A real SHA-256, but one pass, no work factor, and `$sha256$` is not a crypt identifier any other tool knows |
| `login` | verified with a local `simple_hash` | No. `simple_hash` was an invented mixing function |
| `chpasswd` | `$5$`/`$6$`/`$1$` | No. The same invented function, wearing the *standard* labels |

`chpasswd` is the worst of the three: an entry it wrote is indistinguishable
*by its label* from a genuine SHA-512 entry, so a future reader that trusted
the label would compute the real algorithm and get a different answer.

### Decision 1 — the rule lives in `posix`, not in a new shared crate

`posix/src/crypt.rs` was already a correct and complete SHA-crypt:
`$1$`/`$5$`/`$6$`, the `rounds=` field, the crypt base-64 alphabet, and
Drepper's published test vectors. The alternative considered was extracting it
into a new root crate, alongside the existing `yamldoc`, `textfmt`, `textfind`,
`byteread`, `randrange` and `sha2`.

*Against the extraction, decisively:* root `sha2/` implements **SHA-256 only**.
`crypt.rs` needs SHA-512 and MD5 as well, so extracting it would have meant
migrating two more cores first — a large change made only to avoid a dependency
that is, on inspection, the correct one.

*For depending on `posix`:* `crypt` **is** a libc function. Real shadow-utils
link against libc and call it. A userspace tool depending on this project's
libc crate is the arrangement every other Unix has, not a compromise. `posix`
is already a workspace member with `crate-type = ["staticlib", "rlib"]` and
`#![cfg_attr(target_os = "none", no_std)]`, so it builds against std on the
host and the three tools' tests run unchanged.

### Decision 2 — a safe Rust API, because the C ABI was not callable

The request recommended calling `crypt_str`. That function does not exist — it
is a *test helper* inside `crypt.rs`'s own `mod tests`. Everything public was
C ABI: `crypt`, `crypt_r`, `encrypt`, `setkey`, raw pointers, NUL termination,
and a `static mut CRYPT_BUF`. Three Rust callers sharing one mutable static is
not an interface; it is a data race waiting for a second thread.

So `crypt.rs` gained a safe section: `Method` (`Md5`/`Sha256`/`Sha512`, each
knowing its own prefix, hash length and salt maximum), `hash_into`,
`setting_into`, `verify` and `stored_method`, all writing into a caller-owned
`HashBuf` with no shared state. The new tests deliberately do **not** take the
existing `CRYPT_TEST_LOCK`; a test that needed it would be evidence of a bug.

`verify(key, stored)` exists as one function on purpose. crypt's
self-describing property means the stored entry *is* a valid setting: re-run
crypt on it and a correct password reproduces it byte for byte. So
verification needs no salt parsing, no method dispatch, and no slicing — which
is exactly where all three tools went wrong. The constant-time comparison now
sits inside `verify`, next to the value it compares against, where a caller
cannot reach past it and compare something else. (`passwd` had its own
`constant_time_eq`; it is deleted.)

### Decision 3 — old entries fail closed

**The detection half is not a judgment call, which is what dissolved the
tradeoff the request expected.** Genuine crypt hash fields are a fixed length
in crypt base-64 — MD5 22 characters, SHA-256 43, SHA-512 86. Every entry this
tree ever wrote carries **64 hex digits**. `stored_method()` therefore
separates the two populations exactly, with no false positive in either
direction. No heuristic, no ambiguity, and nothing for the operator to
arbitrate.

That leaves only the policy: what to do with an entry once identified as
unverifiable.

| Option | *What changes* |
|---|---|
| **Fail closed (chosen)** | Those users cannot log in until root runs `passwd <user>`. `login` prints a message naming that command. |
| Compatibility path | Those users log in as before, and `login` silently rewrites their entry in the real format on the way through. |

Chosen fail closed, because the compatibility path requires keeping the
invented non-hash alive *inside the authentication path* — the one place in the
system where a wrong answer is a breach — and requires `login` to rewrite
`/etc/shadow` opportunistically, which is a privilege question (`login` at that
moment has not yet dropped to the user) and an atomicity question (a rewrite
interrupted by power loss, against the file that gates all logins). Neither
cost buys anything except avoiding a `passwd` command. This tree has never
shipped, so the population of affected entries is developer test accounts.

The message is explicit rather than a generic "Login incorrect", because the
failure is an administrator's to fix and an unexplained permanent rejection is
the kind of thing that gets diagnosed as a broken keyboard. `passwd` prints the
equivalent when asked for an old password it cannot check.

### Two further bugs found in `login` while in the file

Both fixed here rather than filed, because both are authentication bypasses:

1. **A cleartext fallback.** `verify_password` split the stored entry on `$`
   and, if the shape did not match, compared the typed password against the
   stored field *directly*. Any entry not in the expected shape authenticated
   anyone who typed its literal contents.
2. **No entry meant no password.** If a user had no `/etc/shadow` line — or the
   file was missing or unreadable — `login` prompted for a password, discarded
   it, and logged the user in. That is an unconditional bypass for every
   account whenever the shadow file is absent. It now counts the attempt,
   prints the same "Login incorrect", records the faillog and continues,
   preserving timing and enumeration parity with a wrong password. Checked
   `init/` and `scripts/` first: nothing creates `/etc/shadow`, so no boot path
   depended on the old behaviour.

Also fixed: `login`'s lock check was an equality test against `!`, so an entry
`!$6$…` fell through to verification with `!` as the salt. It is a prefix test
now.

### Two salt bugs, in the tools that generate them

- `passwd` generated **32 hex characters**. SHA-crypt truncates a salt over 16
  characters when hashing but stores what it was given, so the entry could
  never verify against itself. The salt length now comes from
  `Method::salt_max()`, so it cannot disagree with the method again.
- `chpasswd` seeded a linear congruential generator with the literal `42` and
  stirred in `/proc/uptime` — a file this OS does not have — plus the pid. On
  the real system, one `chpasswd` run therefore salted **every account in its
  input identically**.

Both tools read `/dev/urandom` now, and **neither has a fallback**: without it
they refuse to write a password and say so. A fallback generator is a salt in
shape only — whatever seeds it is public, so the whole salt follows from it,
and one precomputed table covers every account salted alongside it, which is
the exact property a salt exists to deny. A password file that cannot be
attacked is worth more than a `passwd` that always succeeds. (`passwd` briefly
kept a day-number fallback for the sake of its own tests on a host with no
`/dev/urandom`; the tests now drive the encoder directly over all 256 byte
values, which is both a stronger test and no reason to weaken production.)

### Why the existing tests passed the whole time

The old tests asserted determinism (`h(x) == h(x)`), difference
(`h(a) != h(b)`), and the shape of the output. **All three are true of any
function written by accident**, which is why they were green while the thing
under test was not a hash. A known-answer vector is the only test that
distinguishes an algorithm from something that resembles one; each of the three
tools now verifies a published Drepper vector, and `login` has a named
regression test that a password set through `passwd`'s code path is accepted by
`login`'s.

**Revisit if** the operator prefers the transparent-rehash path after all
(recorded in `open-questions.md`), or if a real user population ever exists
whose entries predate this change — at which point a one-shot offline
converter, run by an administrator with the file quiescent, is the safe shape,
not a rewrite from inside `login`.
