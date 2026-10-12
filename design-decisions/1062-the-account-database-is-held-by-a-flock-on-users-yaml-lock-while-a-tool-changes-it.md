## 1062. The account database is held by a `flock` on `users.yaml.lock` while a tool changes it

**Date:** 2026-10-07
**Lane:** B
**Decided by:** Claude (autonomous)

**In short:** several programs change the account database:
- `passwd`, `chpasswd`, `chage`;
- `useradd` and its family;
- `useradm`;
- the login manager.

Each reads the whole file, changes it in memory and writes the whole file
back. If two of them run at the same moment, both read the same old
contents, and the one that saves second silently erases the other's
change. shadow-utils prevents that with lock files; this database had no
protection at all. Now every tool that changes it first takes an exclusive
lock, held until its save is done, and a second tool waits for it.

### Options

| | Mechanism | *What changes* |
|---|---|---|
| **A** *(chosen)* | An advisory `flock` (`File::try_lock`) on `/etc/users.yaml.lock`, a file beside the database that is never removed | a second tool waits until the first has saved; a crashed holder's lock dies with it |
| B | shadow-utils' lock files: create `/etc/passwd.lock` holding the pid, `link` it into place, remove it on release | the same waiting, but a crash leaves a lock behind, which the next tool must judge stale by checking whether the pid is still alive |
| C | `flock` on `/etc/users.yaml` itself | does not work: every save replaces the file by a rename, so the second tool would lock the new file while the first still holds the old one |

### Why A

- **A crash cannot wedge the accounts.** The kernel drops a `flock` when its
  holder dies (lane A's advisory-lock table releases on process teardown).
  A lock *file* outlives a crash, and shadow-utils needs pid-liveness checks
  to recover from that.
- **Advisory is enough**, because every writer of the database goes through
  `userdb`, and `userdb::UserDb::lock` is the only door.
- **A separate, permanent file** is what makes `flock` safe here (option C's
  failure), and removing it on release would reopen the same race: a third
  tool could lock a fresh file while a second still waits on the old one.

### The details

- **How long a tool waits:** shadow-utils' 15 tries, a second apart
  (`commonio_lock`). Then it says
  `cannot lock /etc/users.yaml; try again later.`, upstream's words with this
  database's name.
- **What does not wait:** a failure that is not another holder (the
  directory is not writable, the caller is not privileged) is refused at
  once. Upstream waits fifteen seconds for those too.
- **Read-only commands take no lock:** `passwd -S`, `chage -l`, and
  `useradm`'s `list`, `info` and `groups`, as upstream's reading commands
  take none.
- **Where:** `userspace/userdb` (`Lock`, `UserDb::lock`). The holders are
  `passwd` (`Accounts::load_held`), `chage`, `useradm` (`load_db_held`),
  `useradd`'s `Database` and `chpasswd`. The login manager is converted
  separately, because its save had a worse problem than the missing lock.

### What it does not cover

`/etc/group` and `/etc/gshadow` are not generated from the database. Only
`useradd`'s family writes them, and it takes this same lock, so two of those
tools cannot interleave. A ported program that edits them directly,
taking `lckpwdf`'s `/etc/.pwd.lock` instead, would not be excluded. None
exists today.
