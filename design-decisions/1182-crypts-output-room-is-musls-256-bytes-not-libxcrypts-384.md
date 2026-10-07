## 1182. `crypt`'s output room is musl's 256 bytes, not libxcrypt's 384

**Date:** 2026-10-06
**Decided by:** Claude (autonomous)
**Lane:** D

**In short:** `crypt_r` writes its result into a buffer the calling program
provides, `struct crypt_data`, whose size the program learned from the C
header it was compiled with. C here is compiled against musl's
`<crypt.h>`, which makes it 260 bytes; Ubuntu's library (libxcrypt) makes
it 32 KiB and lets a result take 384. Writing more than 260 would corrupt
the caller's memory, so ours lets a result take 256. Every real password
hash fits -- the longest yescrypt one is 183 bytes -- and the only setting
this refuses that Ubuntu takes is an scrypt one with a salt of over 150
characters, which nothing writes.

**The choice:**

| Option | *What changes* | For | Against |
|---|---|---|---|
| **A. 256 bytes** (chosen) | yescrypt's test before hashing -- room for all of the setting, `$`, 43 characters and a NUL -- refuses a setting of 212 bytes or more (`ERANGE`) | safe for every caller compiled against the header C here actually sees; every hash any `crypt` writes verifies | differs from libxcrypt for `$7$` settings of 212-339 bytes, which libxcrypt hashes |
| B. 384 bytes, libxcrypt's | none of A's refusals | libxcrypt's answers exactly | `crypt_r` would write up to 124 bytes past a musl-sized `crypt_data`: memory corruption in the caller, for any setting long enough |
| C. 384, with an overlay `<crypt.h>` declaring libxcrypt's 32 KiB `crypt_data` | B, safely | libxcrypt's ABI and answers | needs the overlay header and every C program rebuilt against it at once -- a binary still holding musl's layout is corrupted -- and is part of a larger piece of work (todo.txt, "libxcrypt's API beyond crypt and crypt_r") |

**Why A, for now.** B is a buffer overflow waiting for a long setting. C is
the right end state, but it is a change of ABI that belongs with the rest
of libxcrypt's API (`crypt_gensalt`, `crypt_rn`, `crypt_ra`, ...), which
programs ported from Ubuntu need anyway. Until then A gives up nothing a
real `/etc/shadow` holds.

**When to revisit:** with the overlay `<crypt.h>` (option C): raise
`CRYPT_OUTPUT_LEN` to 384 in the same change, and delete the oracle
harness's note leaving the 212-339-byte settings out.

**Where:** `posix/src/crypt.rs` (`CRYPT_OUTPUT_LEN`; the module docs' "The
output's room"); `posix/src/yescrypt.rs` (`crypt`'s test);
`posix/tools/oracle/crypt_harness.py` (the cases it leaves out).
