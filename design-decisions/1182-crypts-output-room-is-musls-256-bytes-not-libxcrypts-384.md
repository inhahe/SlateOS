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

**Update, later on 2026-10-06: the header is C's, the room stays A's.** The
rest of libxcrypt's API came, and with it `posix/include/crypt.h`, which
declares libxcrypt's 32 KiB `struct crypt_data` in place of musl's -- C
compiled with the overlay now allocates libxcrypt's struct. The room did
not follow, because the overlay is not the only way to compile C against
this library: it is `-I posix/include` on the compile line
(`scripts/lib/worktree.sh`'s wrappers, the rootfs recipes), not part of the
sysroot, so a compile without it -- and any object built before the header
-- still has musl's 260 bytes. Static linking does not save such an object:
relinked against a newer `libc.a`, it gets the newer `crypt_r`. The two
layouts share the first 256 bytes of the struct, and A writes only there,
so it is safe under both; C's 384 is safe only where musl's header cannot
be seen. What A costs is unchanged: the `$7$` settings of 212-339 bytes.

**When to revisit:** when no compile can see musl's `<crypt.h>` -- the
sysroot installs `posix/include` ahead of musl's headers, say, so that the
overlay is not something a build has to remember. Then raise
`CRYPT_OUTPUT_LEN` to 384 and delete the oracle harness's note leaving the
212-339-byte settings out.

**Where:** `posix/src/crypt.rs` (`CRYPT_OUTPUT_LEN`; the module docs' "The
output's room"); `posix/include/crypt.h` (the struct); `posix/src/yescrypt.rs`
(`crypt`'s test); `posix/tools/oracle/crypt_harness.py` (the cases it leaves
out).
