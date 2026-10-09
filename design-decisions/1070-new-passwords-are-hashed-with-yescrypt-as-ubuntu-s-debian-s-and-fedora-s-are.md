## 1070. New passwords are hashed with yescrypt, as Ubuntu's, Debian's and Fedora's are

**Date:** 2026-10-09
**Lane:** B
**Decided by:** Operator (answering B-Q24: option A, which Claude
recommended). Relayed verbatim by lane A from the operator's answers file,
`open-questions/answers.txt` in the integration tree (lane A's copy:
`operator-answers/2026-10-09-open-questions-answers.txt`).

**In short:** SlateOS never stores a password, only a scrambled form of it (a
*hash*) that can check a typed password but cannot be turned back into one.
Whoever steals the account file can still try guesses against the hashes on
their own hardware, as fast as the scrambling method allows. New passwords
were scrambled with SHA-512 crypt, which a graphics card tries billions of
times a second. From now on they use *yescrypt*, the method Ubuntu (since
22.04), Debian (11) and Fedora (35) use: every guess needs 16 MiB of memory,
which is what makes guessing in bulk expensive. Passwords already set keep
working, each with its own method, until they are next changed.

**The operator's answer, verbatim:** `B-Q24: A`. (The file also has a line
`B-Q24: Claude's recommendation` higher up, which may have been meant for
B-Q23; for B-Q24 the two agree, since Claude recommended A. Lane A has asked
the operator about B-Q23.)

### What changed

- `userdb::PASSWORD_METHOD` is yescrypt. Every program that sets a password
  goes through `userdb` (design-decisions §329, §353): `passwd`, `useradm`,
  the login manager's built-in accounts, and `chpasswd` without `-c` (its
  deliberate difference 3) -- and whatever sets one through the crate later.
- A new password's setting is `crypt_gensalt`'s, asked as PAM asks it for
  Ubuntu's `passwd`: the method's prefix, cost 0 for the default (`j9T`:
  N = 4096 blocks of r = 32, 16 MiB), 16 random bytes
  (`userdb::new_setting`, `SALT_BYTES`). It comes from the C library through
  `libcall::crypt` -- `libc.a`'s on SlateOS, libxcrypt's on a Linux host --
  as `chpasswd`'s settings already did, because a yescrypt salt has a rule
  SHA-512's did not (its characters must decode to whole bytes) and the
  library is the one place that writes them. The random bytes still come from
  `/dev/urandom`, read exactly, with no fallback to a guessable salt.
- `authlib`'s timing cover -- what checking a password costs, spent for a user
  who does not exist (§1061) -- burns a yescrypt check when the store has no
  entry to imitate (`DUMMY_SALT`, 22 characters, the length of a real salt).

### What a check costs (from B-Q24)

| Stored entry | One check | Memory |
|---|---|---|
| `$6$` (SHA-512, the method until now) | ~2.5 ms | negligible |
| `$y$j9T$` (yescrypt, the method now) | ~16 ms | 16 MiB |

Unnoticeable at a login prompt. A server checking many passwords at once
feels it, which is the same property that makes it expensive to attack.

### Where it lives

`userspace/userdb/src/lib.rs` (`PASSWORD_METHOD`, `SALT_BYTES`,
`new_setting`, `Record::set_password`); `userspace/authlib/src/lib.rs`
(`DUMMY_SALT`); the tests that pin the shape of a new entry in `userdb`,
`passwd`, `login` and `chpasswd`.

### How to reverse

Set `PASSWORD_METHOD` back to `Method::Sha512` and `DUMMY_SALT` to a SHA-crypt
salt; `new_setting` follows the method by itself (its prefix and the library's
default cost). The tests that pin `$y$j9T$` fail until they are changed with
it, which is the point of them. Entries written meanwhile keep verifying.
