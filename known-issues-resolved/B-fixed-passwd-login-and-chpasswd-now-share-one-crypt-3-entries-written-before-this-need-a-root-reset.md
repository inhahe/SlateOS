## [B] FIXED — `passwd`, `login` and `chpasswd` now share one `crypt(3)`; entries written before this need a root reset

**Fixed:** 2026-08-17 (lane B). Closes lane C's report above and
`requests/c-b-passwd-and-login-disagree-about-etc-shadow.md`. Rationale and
the alternatives considered: `design-decisions.md` §329. The one remaining
policy choice is `open-questions.md` B-Q3.

**What changed.** `posix/src/crypt.rs` gained a safe Rust API — `Method`,
`hash_into`, `setting_into`, `verify`, `stored_method` — and all three tools
call it. They now agree by construction: a password set with `passwd` is
accepted by `login`, which has a named regression test
(`test_a_password_set_by_passwd_is_accepted_by_login`). New passwords are
SHA-512 (`$6$`).

Lane C's recommended entry point, `crypt_str`, does not exist — it is a test
helper inside `crypt.rs`'s own `mod tests`. Everything public was C ABI over a
`static mut CRYPT_BUF`, which three Rust callers cannot share safely, hence the
new safe section rather than a caller-side wrapper.

**Operational consequence — read this if a login stops working.** Any account
whose `/etc/shadow` entry was written by the old code (`$sha256$…`, or a
`$5$`/`$6$`/`$1$` entry whose hash field is 64 hex digits) can no longer be
logged into. This is deliberate, not a regression: those entries were never
verifiable by anything. `login` prints a message naming the fix, and the fix is

```
passwd <username>          # as root
```

Genuine and bogus entries are distinguishable with certainty — real hash fields
are 22/43/86 crypt-base-64 characters, the bogus ones are 64 hex digits — so
`stored_method()` separates them with no false positives in either direction.

**Two authentication bypasses found in `login` while fixing this, both now
closed.** Neither was in lane C's report; both were worse than the bug that was:

1. `verify_password` fell through to a **cleartext comparison** whenever the
   stored entry did not split into the expected `$`-delimited shape. Anyone who
   typed the entry's literal contents was authenticated.
2. A user with **no `/etc/shadow` entry was logged in without a password** —
   which, when the file itself was missing or unreadable, meant *every* user.
   `login` prompted, discarded the answer, and proceeded.

Also: the lock check was `hash == "!"` rather than a prefix test, so `!$6$…`
was verified with `!` as the salt.

**Two salt bugs, also fixed.** `passwd` emitted 32 hex characters — twice
SHA-crypt's 16-character maximum, which stores what it is given but truncates
what it hashes, so its own entries could not verify against themselves.
`chpasswd` seeded an LCG with the literal `42` plus `/proc/uptime` (a file this
OS does not have) and the pid, so on the real system a single `chpasswd` run
gave **every account in its input the same salt**.

**Neither tool has a fallback when `/dev/urandom` cannot be read; both refuse
to write a password at all.** `passwd` briefly kept a day-number generator for
that case, which is a salt in shape only — the day is public, so the whole salt
follows from it and one precomputed table covers every account changed that
day, which is the exact property a salt exists to deny. It was there only
because the development host has no `/dev/urandom`; the tests drive
`encode_salt` over all 256 byte values instead, so production code no longer
carries a test affordance.

### Notes for whoever reads this next

- **Why the old tests never caught any of this.** They asserted determinism
  (`h(x) == h(x)`), difference (`h(a) != h(b)`), and output shape — all of
  which are true of any function written by accident. A known-answer vector is
  the only test that separates an algorithm from something that resembles one.
  Each tool now checks a published Drepper vector. *Worth applying to any other
  transcribed algorithm in this tree* — see lane C's §4 SHA-256 audit; `passwd`,
  `chpasswd` and `login`'s hand-rolled copies are deleted, and
  `userspace/backup`, `pkg`, `rsync`, `ssh` and `useradm` still carry
  unvectored ones.
