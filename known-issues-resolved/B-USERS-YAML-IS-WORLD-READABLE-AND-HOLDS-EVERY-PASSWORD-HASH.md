## B-USERS-YAML-IS-WORLD-READABLE-AND-HOLDS-EVERY-PASSWORD-HASH (lane B, 2026-09-06) — FIXED 2026-09-06

**In short:** `/etc/users.yaml` holds every account's password hash and is
created with ordinary permissions, so any local user can read it and grind the
hashes offline at their leisure. That is the exact attack the `/etc/passwd` /
`/etc/shadow` split was invented to prevent, and this file reintroduces it.

**Where.** `userspace/userdb/src/lib.rs` → `UserDb::save`. The YAML is staged
with `Access::Shared`, i.e. `std::fs::write`'s default — `0666 & ~umask`, which
is `0644` on any normal system. The generated `/etc/shadow` written beside it
*is* restricted (`Access::Private`, `0600`), so as of §353 step 2 the machine
has the hashes in one protected file and the same hashes in an unprotected one.

**Why it is not simply fixed to `0600` in the same change.** Two unprivileged
programs read the YAML directly today for name-to-uid lookups:

| Program | What it wants | Runs as |
|---|---|---|
| `userspace/chown` | name → uid/gid | whoever typed `chown alice f` |
| `userspace/chroot` | name → uid/gid/home | ditto (`--userspec`) |

Restricting the file today makes `chown alice f` fail for everyone but root.
The other consumers — `authlib`, `doas`, `su` — are already privileged and
would not notice.

**The fix, now that it is reachable.** §353 step 2 generates `/etc/passwd`,
which carries exactly the fields those two want and is world-readable by
design. Move `chown` and `chroot` onto `pwdb` reading `/etc/passwd` (the crate
`id`, `ls`, `tar` and `getent` already use), then change the YAML's staging in
`UserDb::save` from `Access::Shared` to `Access::Private`. Both halves are
small; they are separate only because the second is unsafe to do before the
first.

**Until then.** The hashes are SHA-512-crypt at 5000 rounds (§329), so this is
an offline-grinding exposure rather than an immediate compromise — but it is a
real one, and it is strictly worse than the system's own `/etc/shadow`, which
is `0600`. It has been true since `userdb` was written; §353 step 2 is what
makes it *fixable*, not what introduces it.

**FIXED (2026-09-06),** in the order the entry prescribes. `chown` and `chroot`
read `/etc/passwd` and `/etc/group` through `pwdb` now, and `UserDb::save`
stages the YAML `Access::Private`. The move fixed a second defect on the way:
both crates were *inventing* the group table — `root`, `admin` and `users` had
fixed ids and every other group name mentioned by any account was numbered from
101 in order of appearance — so `chgrp audio` set a gid that depended on which
accounts existed and that no other program on the system agreed with.

The only remaining unprivileged reader would be a new one, and there is now a
reason in `UserDb::save` for the next person who considers adding it.
