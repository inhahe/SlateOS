## §330 — `/etc/users.yaml` gets one implementation, `userdb`, which preserves fields it does not understand

**Date:** 2026-08-17
**Decided by:** Claude (autonomous)

**In short:** Two programs write the file that lists the machine's user
accounts, `/etc/users.yaml`: the account-management tool `useradm` and the
login screen. Each had written its own reader and its own writer, and the two
disagreed — about what the password's *salt* field is called, about what
exactly gets scrambled, and about which fields exist at all. A password set
with `useradm passwd` therefore could not be used to log in, and whichever
program saved the file last deleted every field the other one had written.
Both now go through one shared crate, `userdb`, which keeps each account's
text as it found it and rewrites only the field it was asked to change. This
is §329's fix applied to the *second* password store — the same class of bug,
found in a different file, three days later.

**Terms.** A *salt* is a per-user random string mixed into a password before
it is scrambled, so that two users with the same password do not get the same
stored entry. `crypt(3)` is the C library function Unix uses to do the
scrambling; its output is *self-describing*, meaning the stored entry names
its own method and carries its own salt, so verifying a password needs no
parsing at all — just re-run `crypt` on the stored entry and compare.
*Round-tripping* a file means reading it and writing it back out unchanged.

### What was actually wrong

| | `useradm` wrote | `init/login` wrote |
|---|---|---|
| Salt field | `salt:` | `password_salt:` |
| What was hashed | the salt's **hex text** + password | the salt's **decoded bytes** + password |
| Home directory | `home_dir:` | `home_dir:` (but `su`/`sudo` read `home:`) |
| Admin flag | `is_admin:` | `is_admin:` |
| On save | rebuilt the file from its own struct | rebuilt the file from its own struct |

Two consequences, both silent:

1. **Neither could verify the other's passwords.** Different field name,
   different pre-image; each concluded the other's entries were wrong
   passwords.
2. **Each save deleted the other's fields.** Both serialised from a struct
   holding only the fields that program modelled, so anything the other had
   written — and anything a future version might write — vanished on the next
   write from either side. The file could not accumulate a field.

### Decision 1 — a new crate, rather than pointing both at `posix::crypt`

§329 resolved the *hashing* half of an identical bug by having three tools
call `posix::crypt`. That is necessary here too, and `userdb` does exactly
that — but it is not sufficient, because `/etc/users.yaml` is a *structured*
file with fifteen-odd fields, where `/etc/shadow` is a colon-separated line.
Sharing only the hash would have left two parsers and two serialisers still
free to disagree about everything else, which is where half the damage was.

The alternative considered was parsing through the root `yamldoc` crate,
which already exists and already preserves comments and formatting — the
property the design spec requires of configuration files. **It does not fit:**
`yamldoc` addresses a value by a path of *mapping keys*, so inside a sequence
of mappings — which is precisely the shape of `users:` — an element has no
name. `users[2].uid` is unaddressable. Extending `yamldoc` with sequence
indices was considered and rejected for now: it is a change to a crate three
lanes depend on, made to serve one caller, and the caller's real requirement
is narrower than general indexing.

So `userdb` keeps each record's raw lines and rewrites in place. It is not a
general YAML editor and does not pretend to be; it is an editor for one file
whose shape is known.

### Decision 2 — a write updates *every* spelling of a field, not the first

The two writers had named the same facts differently: `home_dir`/`home`,
`is_admin`/`admin`, `avatar_path`/`avatar`, `password_salt`/`salt`. Records
in the wild therefore carry one spelling, the other, or both.

| Option | *What changes* |
|---|---|
| Write the canonical spelling only | A record that had the alias ends up with both, saying different things; a reader that consults the alias acts on the stale value. |
| Write the first spelling found | Same, mirrored. |
| **Write every spelling present (chosen)** | A record can never contradict itself. A record with neither gets the canonical name. |

Chosen the third. Preserving a field the program does not understand is the
whole point of the crate, and a preserved field that has been allowed to go
stale is worse than a deleted one — it looks current. `set_any` therefore
updates all matching keys and appends the canonical name only when none was
present.

The aliases are **not** normalised away on save. Rewriting a spelling is a
change to a line the caller did not ask to change, and the five read-only
parsers not yet migrated (`su`, `sudo`, `polkit`, `chown`, `chroot`) still
look for the old names; silently renaming their field would break them at the
moment the file was touched by something else entirely.

### Decision 3 — a file that cannot be read is an error, not an empty database

Both writers had `Err(_) => Vec::new()`. So a permission error, an I/O error
or a full disk produced an *empty* user database, which the next save then
wrote over the real file. One unreadable read deleted every account on the
machine. `UserDb::load` now returns an empty database only for
`ErrorKind::NotFound` and propagates everything else.

The login screen's version of this was worse than data loss: on an unreadable
file it fell back to its **built-in default accounts**, which include a root
account whose password is in the source. A permission error opened the machine
up. It now yields no accounts, which fails closed.

`UserDb::save` writes a sibling temporary and renames it over the target. A
truncated `/etc/users.yaml` is a machine with no accounts, and the previous
code truncated in place.

### Decision 4 — the built-in accounts keep a fixed salt, and nothing else may

§329 established that a salt with a fallback generator is a salt in shape
only, and that a tool without `/dev/urandom` must refuse to set a password
rather than invent one. `userdb` holds to that.

The one exception is the two built-in accounts the login screen synthesises
when no database exists — `root` and `guest`. They are salted with a literal
constant. This is not a weakening: their passwords are published in the source
of this repository, so there is nothing a salt could protect. Making them
require `/dev/urandom` would mean a machine with no entropy source cannot
present a login screen at all, which trades a real failure for an imaginary
protection.

### Why the existing tests passed the whole time

The same reason as §329, and it is worth stating twice because the two code
bases were written months apart and arrived at the same non-test
independently. The tests asserted that hashing was deterministic, that
different passwords hashed differently, and that the output was 64 hex
characters. **Every one of those is true of any function written by
accident.** A known-answer vector is the only test that distinguishes an
algorithm from something that resembles one.

So the deleted tests are not ported. Reconstructing them against the new code
would reconstruct the blind spot. `userdb` verifies against `posix::crypt`'s
published vectors, and each migrated tool has a named test that a password set
through its own code path is accepted through the shared one.

### Five further defects fixed while migrating

Found by the rewrite rather than looked for; all in `useradm` unless noted:

1. `userdel` guessed the home directory as `/home/<name>` instead of reading
   the record's own `home_dir`, so a relocated home was left on disk while an
   unrelated path was considered for deletion.
2. `useradd --uid` accepted a non-numeric value and a duplicate uid.
3. `usermod --admin` set the flag but not the `admin` group. The flag is what
   the login screen and the settings app read; the group is what `sudo` reads.
   Setting one made the machine disagree with itself about who is an
   administrator. Both are set now.
4. `useradm list` truncated display names by **byte**, panicking on a
   multi-byte name at the boundary. It truncates by character.
5. (`init/login`) A passwordless account could lock its screen and never
   unlock it — the unlock path required a password the account did not have.
   Refusing did not protect the session; it stranded it, so a locked guest
   screen was a dead end that only a reboot cleared.

And in the authentication path itself: `login` reported a *locked* account and
an account with an unverifiable stale hash both as "Invalid password", which
meant repeated attempts against an already-locked account extended its
lockout. The four outcomes are now distinct.

**Done, 2026-08-17/18.** All five readers now go through the crate:
`init/login` (`3a3321a76`), `userspace/su` and `userspace/sudo`
(`c49964b56`, `65dca4eba`), `userspace/polkit` (`da340eb15`),
`userspace/chown` and `userspace/chroot` (`e101e6d04`). Every one of them was
a bug fix rather than housekeeping — none was reading a file any writer of
this tree had ever produced:

| Crate | What its own parser did |
|---|---|
| `su` | read `home:`; both writers write `home_dir:` |
| `sudo` | discarded the password entirely, and admitted *everyone* when the file was absent |
| `polkit` | read `admin:`; both writers write `is_admin:`, so it saw a machine with no administrators |
| `chown`, `chroot` | read only `uid`/`username`/`groups`, so `is_admin` never became group membership |

Three defects were common to more than one of them and are now fixed once, in
the crate or in the callers:

1. **An administrator is a member of `wheel`.** The database records
   administrator-ness as a flag; every policy file in the wild — sudoers,
   polkit rules, `chgrp wheel` — is written against the group. `sudo`,
   `polkit`, `chown` and `chroot` now fold the flag into the group list.
2. **`home_dir` is the home directory.** `polkit`'s `pkexec` composed
   `/home/<name>`, so a command run as root got `/home/root`.
3. **A password test that never serialises proves nothing.** `polkit`'s six
   password tests all passed against a hash function no writer used. Each
   migrated crate now has at least one test that writes the database out and
   reads it back, which is the only step at which a reader and a writer that
   disagree can be seen to disagree.

**Revisit if** `yamldoc` gains addressing for sequence elements, at which
point `userdb`'s line-level record editing could sit on top of it and inherit
comment preservation for free; or if the operator settles the open question of
whether `/etc/users.yaml` or `/etc/shadow` is the system's one account
database, in which case one of the two stores — and one of §329 and §330 —
becomes redundant.
