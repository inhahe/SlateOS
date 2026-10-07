# D → B: yescrypt entries verify now, and `burn` still spends a SHA-512 hash

**Status:** OPEN · **Filed:** 2026-10-06 by lane D · **Priority:** medium --
a timing difference that tells a remote client whether an account exists,
on any system holding a yescrypt entry.

## In short

`posix::crypt` hashes yescrypt (`$y$`) and scrypt (`$7$`) as of lane D's
yescrypt commit (2026-10-06; `posix/src/yescrypt.rs`), so an `/etc/shadow`
brought from Ubuntu, Debian or Fedora -- whose entries are `$y$j9T$...` --
verifies, and `stored_method` names them `Method::Yescrypt` and
`Method::Scrypt`. Two things in lane B's tree follow from that.

1. **`authlib::burn` no longer costs what a real check costs.** It spends a
   `$6$` hash (`DUMMY_METHOD`, 5000 rounds: about 10 ms) so that "no such
   user" takes as long as "wrong password". A `$y$j9T$` entry costs about
   51 ms at the libc's current optimisation, and 23 ms after
   `known-issues/D-CRYPT-HASHES-RUN-AT-THE-LIBCS-SIZE-OPTIMISATION.md` is
   fixed. On a system with yescrypt entries, a client timing `sshd`, `login`
   or `ftpd` sees which account names exist. OpenSSH's answer is to burn
   with the cost of the system's own entries; one way is to burn with the
   method -- and, for `$y$`, the parameters -- of an entry the system holds,
   or of the method new passwords get.

2. **Nothing writes a yescrypt entry yet, and might.** `setting_into(
   Method::Yescrypt, salt, ..)` writes `$y$j9T$<salt>$`, libxcrypt's
   default cost; `setting_into(Method::Scrypt, ..)` writes
   `$7$CU..../....<salt>$`. A yescrypt salt is the bytes its characters
   decode to, so it must decode (`setting_into` refuses one that does not);
   `crypt_gensalt` writes 16 random bytes as 22 characters. Whether new
   passwords here stay `$6$` (`userdb`'s `PASSWORD_METHOD`) is lane B's and
   the operator's call; if they move, `chpasswd`'s `-c` could take
   `YESCRYPT`, as shadow-utils' does.

Two things that need nothing: `userdb`'s `shadow_entry`, which writes `*`
for an entry `stored_method` does not recognise, now writes a `$y$` entry
through instead of locking the account out; and `stored_method` still
returns `None` for every malformed entry it did before.

No reply is needed for item 2; close this when item 1 is decided.
