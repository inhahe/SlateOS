## §347 — The failure tally is one shared file with fixed slots, not one file per user

**Date:** 2026-08-21
**Decided by:** Claude (autonomous)

**In short:** When you type a password wrong, the system counts it and makes
you wait a little longer before the next try — one second, then two, then four,
up to five minutes. Until now that count lived only in the memory of the
program asking, so a program that runs once and exits — `doas`, the "run this
one command as root" tool — forgot the count the instant it ended. Guessing at
its prompt was therefore free: fail, exit, run it again, no delay. The count
now lives in a file, so it survives. This records the three ways that file
could have been built and why it is one shared table rather than the obvious
one-file-per-user.

**Related:** §341 (`authlib` exists at all), §329 (the three disagreeing
hashers that led to it), §346 (the two callers that resolve `NoPassword`
oppositely), `known-issues.md` →
`B-DOAS-COULD-NOT-VERIFY-ANY-PASSWORD-THE-SYSTEM-ACTUALLY-SETS`.

### The decision, in four parts

**1. The tally is shared between programs, not per-program.**

The alternative was to give each program its own file — `doas` already keeps
per-uid state under `/var/run/doas` for its `persist` timestamps, so the shape
existed. Rejected: an attacker guessing a password does not care which prompt
answers. Three separate tallies means three independent budgets of free
guesses, and the escalation restarts every time they switch prompts. The count
belongs to the *account*, which is what is under attack, not to the program
that happened to notice.

The cost, and it is real: a failure in one program now delays the user in
another. Someone who fat-fingers `doas` three times is briefly slowed at the
next `ssh`. That is `pam_faillock`'s behaviour on Linux and is what "one tally
per user" means; it is not a side effect to be engineered away.

**2. One fixed-slot file, not one file per user.**

This is the part that was not obvious, and it is a security decision rather
than a tidiness one. A username at a login prompt is *attacker-chosen text*. A
scheme that names a file after it hands an unauthenticated caller two
primitives at once:

- **Path traversal.** `../../etc/passwd` is a valid thing to type at `login:`.
  Escaping it correctly is possible; getting it wrong is a root-owned write to
  a path the attacker picked, and the escaping has to stay correct forever.
- **Unbounded file creation.** Every invented name creates a file. A loop
  typing random names fills the inode table of whatever filesystem `/var/run`
  is on, which is a denial of service against every program that needs to
  write, not just against authentication.

One file with `MAX_SLOTS = 1024` rows removes both. Usernames are hex-encoded
inside it, so no name — including one containing a space or a newline, the
field and row separators — can forge or corrupt a neighbouring row. That is
asserted by `a_username_cannot_inject_rows_into_the_table`.

**3. An invented username occupies a slot exactly as a real one does.**

Skipping names with no account would have been simpler, would have made the
slot pressure in part 2 mostly theoretical — and would have leaked precisely
the thing the rest of the design works to hide. If unknown users are not
recorded, then "am I rate-limited?" answers "does this account exist?", and an
attacker enumerates the user list by watching which names slow down. `authlib`
already burns equal time on the no-such-user path for the same reason; a tally
that treated the two differently would have undone that at a different layer.

**4. When the table is full, evict the row with the *fewest* failures.**

The naive choice is oldest-first (LRU), and it is exactly wrong here. Under
attack, the attacked account is the row being written most often — under LRU it
survives, but so does every junk row, and 1024 junk names still push it out
eventually. Evicting by fewest-failures-first, oldest as the tie-break, means
the flood of one-failure junk names consumes itself: the attacker's own rows
are always the cheapest to discard, and the account with ten failures is the
last thing standing. `flooding_the_table_does_not_evict_the_account_under_attack`
is that property.

### Two smaller calls recorded with it

**`/var/run`, not `/var/lib`.** The tally describes an attack *in progress*,
not a durable fact about the account, and the delay is capped at five minutes.
State that a reboot clears is correct here — and an attacker cannot arrange a
reboot more cheaply than waiting out the five minutes, so nothing is bought by
crossing one.

**Combining memory and disk takes the maximum of each field separately.**
`combine` takes the larger `failures` *and* the later `last_failure_secs`,
rather than preferring one row wholesale. Preferring the disk row would let a
stale file shorten a delay the running process had already earned; preferring
memory would let a fresh process ignore what another program recorded a second
ago. Field-wise maximum has the property that matters: neither half can
*shorten* the other's delay, only lengthen it.

**The directory's mode is forced, not inherited.** `create_dir_all` applies
the process umask, and a setuid program inherits the umask of whoever ran it —
so a user with `umask 0` who happened to be the first to trigger a write would
get a world-writable `/var/run/authlib`, and could then rename their own file
over the tally to clear their failures. No permission on the *file* can prevent
that; the directory is what has to be 0700, and it is set explicitly rather
than left to whatever the caller's environment happened to be.

**A failed write is ignored.** `write_shared` discards the error, which is the
kind of thing this project normally forbids. The justification is that the
in-memory tally still limits the running process, so an unwritable `/var/run`
degrades to exactly the behaviour that existed before this change. The
alternative — refusing to authenticate when the tally cannot be written — turns
a full disk into a machine nobody can log into, which is a worse failure than
the one it prevents.

### The half left open

`login` and `su` still do not share the tally: `login` calls the checking half
of `authlib` directly because it owns the console's empty-password policy
(§346), and `su` predates `authlib`. Closing that needs `authlib` to expose the
counting half on its own — `rate_limited` / `note_failure` around the caller's
own verdict — which is straightforward.

What is not straightforward, and is deliberately not decided here, is the
consequence: once `login` shares the tally, any unprivileged process running as
the user can hold that user at a delayed *console* prompt by failing `doas` on
purpose. It is bounded (five minutes, never a lockout) and it is what Linux
does, but "any local process can add five minutes to your console login" is a
user-visible policy, not an implementation detail. It is written up in
`known-issues.md` → "Still open — `login` and `su` do not share the tally",
and queued for the operator as `open-questions.md` → **B-Q6**, which lays out
four answers. Note that `su` sharpens it: `su` guesses at the *target's*
password, so if `su` joins too, any local user could hold **root** at a delayed
console prompt without ever having had root.
