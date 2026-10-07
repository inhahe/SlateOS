## D-CRYPT-SHA-ROUNDS-WERE-CLAMPED-WHERE-LIBXCRYPT-REFUSES — `crypt` read a SHA-crypt `rounds=` field as glibc's old crypt did, clamping a count out of bounds and taking a malformed field as salt, where libxcrypt refuses both (lane D, 2026-10-07) — FIXED 2026-10-07
**Status:** FIXED 2026-10-07 — `crypt.rs`'s `sha_rounds`, held to libxcrypt by 60 more oracle answers

**In short:** a SHA-256 or SHA-512 crypt setting may say how many rounds
to hash with: `$6$rounds=5000$salt`. Ubuntu's library (libxcrypt) takes
that field only when it is a plain number from 1000 to 999999999 followed
by `$`; anything else -- `rounds=100`, `rounds=01000`, `rounds=abc`, a
missing `$` -- is refused with `EINVAL`. This library answered as glibc's
old `crypt` did instead: it raised `rounds=100` to 1000 and hashed, and
took `rounds=abc$salt`, or `rounds=100` with no `$` after it, as the salt.
So the same setting hashed here and failed on Ubuntu -- and worse, a hash
made from `$6$rounds=100` (salt `rounds=100`) could not be verified by
this library itself, since its own output read back as a rounds field.

**How it was found.** By `posix/src/crypt_fuzz.rs`, the hostile-input test
written the same day, whose first run mutated `$6$rounds=1000$saltstring$`
into `$6$rounds=100`, hashed it, and found that the hash neither named its
method (`stored_method`) nor verified. The oracle had never asked: its SHA
cases were libxcrypt's known-answer table's, whose `rounds=` fields are
all valid.

**The fix.** `sha_rounds` reads the field as libxcrypt's `crypt-sha256.c`
and `crypt-sha512.c` do -- a digit 1-9 first, the count within the bounds,
then `$`, else `EINVAL` -- and `sha_crypt` refuses rather than falling
back; `stored_method` refuses such an entry, which can never verify.
`setting_rounds_into` still clamps the count it is asked for, as it must
write one `crypt` takes. The oracle (`crypt_harness.py`) now asks
libxcrypt about 26 fields for each of `$5$` and `$6$`, and MD5 crypt's
salt edges, and `crypt.rs`'s `rounds_fields_libxcrypt_refuses_are_refused`
states the rule.

**What it could have cost.** An entry with a malformed or out-of-range
count in `/etc/shadow` -- written by hand, or by a tool that clamps -- was
hashed here and refused on Ubuntu, so the same account logged in on one
and not the other; and the fuzz test's case, a hash that its own library
cannot verify, would have locked its account out.
