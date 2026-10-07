# D → B: yescrypt and bcrypt entries verify now, and `burn` still spends a SHA-512 hash

**Status:** OPEN · **Filed:** 2026-10-06 by lane D · **Priority:** medium --
a timing difference that tells a remote client whether an account exists,
on any system holding a yescrypt or bcrypt entry.

## In short

`posix::crypt` hashes yescrypt (`$y$`), scrypt (`$7$`) and bcrypt (`$2a$`,
`$2b$`, `$2x$`, `$2y$`) as of lane D's commits of 2026-10-06
(`posix/src/yescrypt.rs`, `posix/src/bcrypt.rs`, the hashing in
`posix/pwhash`), so an `/etc/shadow` brought from Ubuntu, Debian or Fedora
-- whose entries are `$y$j9T$...` -- or from OpenBSD or an old openSUSE
verifies, and `stored_method` names them `Method::Yescrypt`,
`Method::Scrypt` and `Method::Bcrypt`. Two things in lane B's tree follow
from that.

1. **`authlib::burn` no longer costs what a real check costs.** It spends a
   `$6$` hash (`DUMMY_METHOD`, 5000 rounds: about 2.5 ms on the development
   machine) so that "no such user" takes as long as "wrong password". A
   `$y$j9T$` entry costs about 16 ms, a `$2b$10$` one about 50. On a system
   with such entries, a client timing `sshd`, `login` or `ftpd` sees which
   account names exist. OpenSSH's answer is to burn with the cost of the
   system's own entries; one way is to burn with the method -- and its
   parameters, for `$y$` and `$2b$` -- of an entry the system holds, or of
   the method new passwords get.

2. **Nothing writes a yescrypt or bcrypt entry yet, and might.**
   `setting_into(Method::Yescrypt, salt, ..)` writes `$y$j9T$<salt>$`,
   libxcrypt's default cost; `Method::Scrypt` `$7$CU..../....<salt>$`;
   `Method::Bcrypt` `$2b$05$<salt>$`. A yescrypt salt is the bytes its
   characters decode to, so it must decode, and a bcrypt salt is exactly 22
   characters whose last holds two bits (`.`, `O`, `e` or `u`);
   `setting_into` refuses others. `crypt_gensalt` writes 16 random bytes as
   22 characters for both. Whether new passwords here stay `$6$`
   (`userdb`'s `PASSWORD_METHOD`) is lane B's and the operator's call; if
   they move, `chpasswd`'s `-c` could take `YESCRYPT` and `BCRYPT`, as
   shadow-utils' does.

Two things that need nothing: `userdb`'s `shadow_entry`, which writes `*`
for an entry `stored_method` does not recognise, now writes such an entry
through instead of locking the account out; and `stored_method` still
returns `None` for every malformed entry it did before.

No reply is needed for item 2; close this when item 1 is decided.
