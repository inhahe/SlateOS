## 1464. A program asking for a password is allowed once or until the vault locks -- never for good

**Date:** 2026-10-05 &middot; **Decided by:** Claude (operator-approved scope:
that a program may ask, with a key and your say-so, is the operator's, §1417;
how the service asks you is Claude's) &middot; **Lane:** C

**In short:** when a program asks the password manager for a password, a
window shows which program is asking and for what, and you can refuse, allow
it this once, or allow it until the password manager locks again. There is
no "always allow". An "always" would have to be written down in a file, and
any file the service can write, your other programs can write too -- so a
program could write itself an "always" and never be asked again, which is
exactly what asking you is there to prevent. It would also save almost
nothing: once the password manager locks, the next ask needs its master
password typed anyway. Two smaller rules come with it: a program you refuse
is refused without asking you again for a minute, and a password saved for a
secure (`https`) address is never handed over for an insecure (`http`) one.

**Where:** `gui/credentials/src/service.rs` (the judgement),
`gui/credentials/src/matching.rs` (the scheme rule).

### The choice offered: once, until locked, or no

| Option | What changes | For | Against |
|---|---|---|---|
| **Once / until locked / refuse** (chosen) | a program allowed "until locked" asks silently until the vault locks, then you are asked again | nothing on disk can be forged; the vault's own auto-lock bounds how long a yes lasts | a program you trust is asked about after every lock -- but the master password must be typed then anyway |
| Once / always / refuse, "always" kept in a plain file | a program allowed "always" is never asked again | one fewer prompt after a lock | any program running as you can write the file: a program holding the key grants itself everything, silently |
| "always" kept in a file sealed with a key derived from the vault's | as above | a forged file is detected | revoking by editing the file breaks the seal (every grant goes); an old sealed file can be put back to undo a revocation; it buys one fewer click per lock |
| "always" kept inside the vault | as above | sealed with everything else | the service would write lane E's vault, which it promised never to do -- two writers of one file lose each other's saves |

What was allowed until locked is kept in the service's memory and forgotten
when the vault locks, by its own timer or by hand, or when the service stops.
It is per program and per login: allowing a mail program your bank login does
not allow it your other bank login, nor allow any other program anything.

### Quiet after a refusal

A program you refuse -- or whose ask ends in three wrong master passwords --
is refused without asking for `QUIET` (sixty seconds). Without it a program
holding the key could pop the prompt up again the moment you closed it, and
keep doing so. Sixty seconds is long enough to stop that and short enough
that a program you refused by mistake can ask again soon. Per program: one
program refused does not quiet another. And it stops prompts only: a login
you allowed the program until the vault locks is still given, since refusing
it something else takes back nothing you allowed.

### A login goes to its own scheme, or up to `https`

Where both the saved address and the asked one name a scheme, they must be
the same -- except that a login saved for `http://` may go to `https://`, the
same place reached more safely. Never the other way: a login saved for
`https://bank.example` is not handed over for `http://bank.example`, a page
anyone on the network in between can rewrite. A saved address with no scheme
(`bank.example`) goes to any.

### Also decided here

- **Which login.** The best match for the address (an exact path beats an
  exact domain beats a parent domain beats a wildcard), among the program's
  named user's logins when it names one. Where several match equally you
  pick, shown each one's address and user name -- never its password.
- **Nothing to give, nothing to ask.** With the vault open, an address it
  holds nothing for is refused without asking you. With it locked you are
  asked first, since nothing can be known before it opens.
- **How long the vault stays open.** As long as the password manager's own
  auto-lock setting says it may sit unused -- the setting you chose for
  exactly this -- each use starting the wait again.
- **Who is asking.** The program is the executable of the process the kernel
  recorded connecting. The service reads that, *then* asks the kernel whether
  the process holds the key; the kernel says yes only while that process
  still holds its end of the connection, so the name read cannot be that of a
  process that took its number after it exited.

**Revisit if:** the vault ever gains a writer the service may share (then
"always" could live inside it, sealed, with a revocation list), or users ask
to be prompted less -- the answer then is a longer auto-lock, which is theirs
to set.
