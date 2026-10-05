## §353 — `/etc/users.yaml` is the truth and the flat files are generated from it, read-only

**Date:** 2026-08-21
**Decided by:** Operator (option C; Claude recommended this option)

**In short:** Two files on this system each claimed to be *the* list of user
accounts — `/etc/users.yaml`, and the classic pair `/etc/passwd` +
`/etc/shadow` — and nothing copied anything between them. Twenty-three programs
read one or the other, so a user created with `useradd` could log in over SSH
but did not exist to the graphical login screen, and a user created with
`useradm` was the reverse. The decision: the YAML file is the real one, and the
two flat files are *generated* from it whenever it changes, for the benefit of
ported software that reads them directly. Both exist and they always agree.

**Answers:** `open-questions.md` B-Q4 (deleted from that file by this entry).
**Related:** §330 (the five broken YAML parsers unified into `userdb`), §329
(the three broken hashers unified), §341 (`authlib`, which is where the guess
currently lives), `known-issues.md` → the two-`sudo`-binaries entry, which this
unblocks.

### Why not either of the single-file answers

**A — YAML wins outright, delete the flat files.** This is what `design.txt`
asks for ("configuration files will be yaml", line 1108) and it is one file
instead of three. It breaks every piece of ported software that calls
`getpwnam()` and then reads `/etc/passwd` itself — and we do not know how many
of those there will be, because the answer arrives with each new port rather
than now. Betting the account system on that unknown is the expensive kind of
guess.

**B — flat files win, delete the YAML.** Ported software works untouched and
administrators know the format. But it contradicts `design.txt` for the most
security-sensitive file on the system, and the desktop's extra fields (avatar,
auto-login, last-login count) have nowhere to live in a colon-separated line —
so they need a second file, which re-creates the exact split this option was
meant to end.

C is the only option that does not choose between `design.txt` and every future
port. And its cost is work option A needs anyway: `useradd` and `passwd` write
accounts, so they must be redirected at the YAML in either case. If C turns out
to be more machinery than it is worth it degrades into A by simply not
generating the flat files — the truth does not move.

### What has to be built

1. **Redirect the two writers.** `useradd` and `passwd` write to the YAML via
   `userdb`, not to `/etc/passwd`/`/etc/shadow` directly. Without this the
   generated files are stale the moment anyone uses them.
2. **Generate on change.** Every `userdb` write regenerates both flat files.
   They must be written the way any critical file is — to a temporary and
   renamed — so a crash mid-generation cannot leave a truncated `/etc/passwd`.
3. **Collapse `authlib`'s guess.** Today `authlib` picks a store per lookup:
   YAML if it has the user, `/etc/shadow` otherwise. That fallback is a policy
   invented in the absence of this decision. It becomes a straight YAML lookup;
   the `/etc/shadow` branch becomes dead and is deleted rather than left as a
   fallback, because a fallback that fires means the generation broke, and
   silently reading a stale account file is worse than failing.
4. **The generated files are read-only in intent, not in permission.** A
   hand-edit of `/etc/passwd` survives until the next `useradm` run and is then
   silently undone. This is the one genuinely surprising thing about C. The
   mitigation is a generated header comment naming the source file, which is
   what every other generated config on this system does.

### The known cost

A file that looks writable and is not surprises people. Accepted: this is what
macOS does — its truth is a database and the flat files are vestigial — and it
has surprised people for twenty years without being the wrong call. The
alternative surprise, two disjoint sets of users, is strictly worse: it has
already produced two of the nastiest defects in this tree, where `sudo` and
`doas` answer "is this person an administrator?" from different files, as do the
two `login`s. A machine could genuinely believe an account was an administrator
at the graphical prompt and not exist at all over SSH.

**Revisit if:** the generation step turns out to be the only thing anyone
maintains — i.e. a year passes and nothing ported ever read `/etc/passwd`
directly. Then stop generating and this is option A, with no other change.
