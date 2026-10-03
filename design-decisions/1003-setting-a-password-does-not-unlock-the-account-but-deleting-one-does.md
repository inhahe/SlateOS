## 1003. Setting a password does not unlock the account, but deleting one does

**Date:** 2026-09-06
**Lane:** B
**Decided by:** Claude (autonomous)

**In short:** an administrator can *lock* an account, which makes it refuse every
password without forgetting the one it had. The question is what should happen if
they then set a new password on that locked account: does the account quietly
start working again, or does it stay locked until someone says otherwise? We now
keep it locked and print a line saying so. The traditional Unix tools unlock it,
but only as an accident of how they stored the lock.

**Where it bites:** `userspace/passwd/src/main.rs`, `cmd_change_password` (the
`still_locked` note) and `cmd_delete_password` (the `set_locked(false)` call).

### Why the traditional behaviour is an accident

`/etc/shadow` has one column for the password, and a lock is spelled by writing a
`!` in front of whatever is already there — `!$6$abc$def…`. There is no separate
"locked" field. So when shadow-utils' `passwd` sets a new password it writes the
new hash over the whole column, and the `!` disappears with it. The account
becomes usable again. Nobody decided that; it falls out of there being one column
where two facts were stored.

Our database has both facts separately: a `password_hash` and a `locked` flag
(`userdb`'s `Record`). Once they are separate, the behaviour has to be *chosen*.

| Option | *What changes:* | |
|---|---|---|
| **Setting a password leaves the lock alone (chosen)** | `passwd dave` on a locked account stores the password, prints "password updated successfully", and then prints "note: `dave' is still locked and will refuse this password; run `passwd -u' to unlock it". Logging in still fails until `-u`. | An administrator who disabled an account cannot re-enable it by accident — including by running the one command that is routinely run on someone else's behalf, at their request, over the phone. |
| Setting a password unlocks, as shadow-utils does | `passwd dave` on a locked account silently re-enables it. | Matches ported habits, and matches what an administrator who learned Unix elsewhere will expect. But the failure it permits is silent and in the unsafe direction: an account that was deliberately disabled starts accepting logins with nothing printed to say so. |
| Refuse to set a password on a locked account | `passwd dave` fails with "account is locked" until it is unlocked. | The safest of the three and the most annoying: the natural order of "give them a new password, then re-enable them" is refused, and the remedy — unlock first — is the very thing the administrator was being careful about. |

**Deleting** a password is the opposite case and goes the other way: `passwd -d`
clears the lock. Here the two facts are not independent — the lock is a lock *over
a password*, and there is no longer a password for it to be over. Leaving the flag
set would produce an account that reports "no password" and refuses every login,
which is not a state anyone asks for; and under the old `!`-prefix spelling the
lock went away automatically when the column was emptied, so keeping it would be
a silent behaviour change rather than a chosen one.

**Against the choice, honestly:** it diverges from shadow-utils, and a script that
locks an account and then sets a password expecting it to come back will now leave
it locked. That is judged acceptable because the divergence fails *safe* — the
account stays disabled rather than quietly becoming usable — and because it is
announced on every run that hits it, rather than being something the administrator
has to already know. `passwd` is also the interactive tool; the scripted path for
setting passwords is `chpasswd`, which does not lock anything.

**If it is never revisited:** nothing degrades. The one cost is the surprise for
someone carrying shadow-utils habits, and the printed note is there to spend
itself on exactly that.
