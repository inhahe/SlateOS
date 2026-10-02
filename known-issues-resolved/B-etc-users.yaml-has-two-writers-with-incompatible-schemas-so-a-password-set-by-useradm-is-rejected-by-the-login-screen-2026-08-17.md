## [B] `/etc/users.yaml` has two writers with incompatible schemas, so a password set by `useradm` is rejected by the login screen (2026-08-17)
**Status:** FIXED 2026-08-17 (`cc0fa5da9`, `5ab46559a`, `3a3321a76`) — both writers go through `userspace/userdb`; see the FIXED section below and design-decisions.md §330.

**In short:** SlateOS keeps its own user database at `/etc/users.yaml`, separate
from the POSIX `/etc/shadow`. Seven programs read it and two of them write it —
`init/loginmgr` (the graphical login manager) and `userspace/useradm` (the account
management CLI) — and the two disagree about what the file looks like. Setting
a password with `useradm passwd` produces an entry the login screen cannot
authenticate against, and each tool silently deletes the fields the other owns
when it rewrites the file. **This is the same bug lane C reported for
`/etc/shadow` (fixed, `design-decisions.md` §329), one level up: same file,
different tools, no agreement, and no test that compares them.**

### The disagreements, measured against the code

| | `useradm` | `init/loginmgr` |
|---|---|---|
| Salt field | `salt:` | `password_salt:` |
| What is hashed | `sha256(hex_text_of_salt ‖ password)` | `sha256(raw_salt_bytes ‖ password)` |
| Avatar | `avatar:` | `avatar_path:` |
| Home | `home:` | `home_dir:` |
| Admin flag | `admin:` | `is_admin:` |
| Only in `useradm` | `groups:`, `locked:` | — |
| Only in `init/loginmgr` | — | `auto_login:`, `last_login_timestamp:`, `login_count:` |

Two independent reasons a `useradm`-set password fails at the login screen:
`init/loginmgr` looks for `password_salt:` and finds only `salt:`, so it hashes
with an *empty* salt; and even given the salt it would hash the decoded bytes
where `useradm` hashed the hex text. Either alone is fatal.

The field-set difference is a data-loss bug in both directions. Each writer
emits exactly its own fields, so `useradm mod` on a database the login manager
wrote drops `auto_login`, `last_login_timestamp` and `login_count`, and the
login manager writing back drops `groups` and `locked` — including the group
memberships that `sudo` and `polkit` make authorisation decisions from.

`init/loginmgr/src/main.rs`: `hash_password` ~379, `authenticate` ~982,
`serialize_users_yaml` ~514, `parse_users_yaml` ~541.
`userspace/useradm/src/main.rs`: `hash_password` ~177, `read_users` ~86,
`write_users` ~144.

### The other five readers

`su`, `sudo`, `polkit`, `chown` and `chroot` each carry their own parser of the
same file — seven hand-written parsers of one format, which is how the two
schemas were able to drift apart without anything failing to compile. They are
read-only, so they cannot corrupt the file, but each silently gets `None` for
any field named the way the *other* writer names it.

### The password hash itself

Both constructions are `sha256(salt ‖ password)` in one pass: no work factor,
so an attacker with the file tries passwords as fast as they can hash, which is
billions per second. `/etc/shadow` no longer has this problem — §329 moved it to
SHA-512-crypt with 5000 rounds via `posix::crypt`. The native database should
use the same implementation; there is no reason for this OS to contain two
password-hash constructions, let alone three.

### Proper fix

One shared implementation of the format — record type, parser, serialiser that
round-trips *every* field including ones the caller does not know about, and
authentication via `posix::crypt` — used by both writers and, in time, the five
readers. This is the §329 fix applied to the second password store.

**Not blocked on the open architectural question** (`open-questions.md`, whether
`/etc/users.yaml` or `/etc/shadow` is the system's one account database):
whichever wins, the tools that write a file today must agree about it today, and
one parser is easier to delete later than seven.

### FIXED, 2026-08-17 (`cc0fa5da9`, `5ab46559a`, `3a3321a76`)

Both writers now go through one crate, `userspace/userdb`. It parses records
into raw lines and rewrites only the field asked for, so neither program can
delete a field it does not model; it writes passwords with `posix::crypt`
(SHA-512-crypt, 5000 rounds) and verifies with `crypt`'s self-describing
property, so the salt-name and pre-image disagreements have nothing left to
disagree about; and where the two writers used different names for the same
fact (`home_dir`/`home`, `is_admin`/`admin`, `avatar_path`/`avatar`,
`password_salt`/`salt`), a write updates **every** spelling the record
carries, so a preserved field cannot go stale. 23 tests in `userdb`, 5 new in
`useradm`, 44 green in `login`. Reasoning in `design-decisions.md` §330.

`hash_password`, `read_users`, `write_users`, `generate_salt`, `sha256_hex`
and the local SHA-256 are deleted from `useradm`; `hash_password`,
`serialize_users_yaml`, `parse_users_yaml`, `sha256`, `bytes_to_hex` and
`hex_to_bytes` are deleted from `init/loginmgr`.

Eight collateral defects fixed in passing, listed in §330 — the two that
matter most: a read failure produced an *empty* database that the next save
wrote over the real file (both writers), and in `login` that same failure
substituted the built-in defaults, which include a root account whose password
is in the source, so a permission error opened the machine up rather than
closing it.

**The five read-only parsers are also done** (`c49964b56`, `65dca4eba`,
`da340eb15`, `e101e6d04`). Not one of them was reading a file any writer of
this tree has produced:

| Crate | What its own parser did | Consequence |
|---|---|---|
| `su` | read `home:`; writers write `home_dir:` | every `su -` landed in the wrong directory |
| `sudo` | never looked at the password at all | **total authentication bypass** — see the entry above |
| `polkit` | read `admin:`; writers write `is_admin:` | every machine looked to it like one with no administrators, so `auth_admin` refused before prompting |
| `chown` | read no admin field | `chgrp wheel` → "unknown group" |
| `chroot` | read no admin field | `--userspec root:wheel` → "unknown group" |

All four crates now fold the `is_admin` flag into the group list, because the
database records administrator-ness as a flag while every policy file in the
wild — sudoers, polkit rules, `chgrp` — names the group. `polkit` additionally
stopped composing `/home/<name>` for `pkexec`'s HOME, which gave a command run
as root `/home/root`.

Test counts after: `su` 46, `sudo` 198, `polkit` 82, `chown` 41, `chroot` 46.
Each now has at least one test that serialises a database and reads it back —
the only step at which a reader and a writer that disagree about the format
can be seen to disagree, and the step none of the replaced tests took.
