## 1061. A password check that checks nothing costs what the most common stored entry costs

**Date:** 2026-10-07
**Lane:** B
**Decided by:** Claude (autonomous), answering item 1 of
`requests/d-b-yescrypt-entries-verify-now-and-burn-costs-a-sha512-hash.md`

**In short:** when someone tries to log in as an account that does not exist,
the system must take as long to say no as it does for a real account with
the wrong password. Otherwise an attacker can tell which account names are
real just by timing the answer. Until now the "no such account" answer
spent the time of one fixed kind of password hash. Accounts brought from
Ubuntu, Debian or Fedora use a slower kind (yescrypt), and those now work
here (lane D, 2026-10-06), so on such a system the fake check was 6 to 20
times quicker than the real one. The fake check now imitates the accounts
the system actually has.

### What a check costs, by method (development machine)

| Stored entry | One check |
|---|---|
| `$6$` (SHA-512, our new passwords) | ~2.5 ms |
| `$y$j9T$` (yescrypt, Ubuntu/Debian/Fedora) | ~16 ms |
| `$2b$10$` (bcrypt) | ~50 ms |

### Options

| | Approach | *What changes* |
|---|---|---|
| A | Keep one fixed `$6$` burn | nothing: a system of `$y$` entries answers "no such user" ~13 ms sooner |
| B | OpenSSH's `pick_salt`: imitate the first account with a `$`-style entry | right whenever every entry is alike; on a mixed system, wrong for every account not in the first one's class |
| **C** *(chosen)* | Imitate an entry of the **most common** cost class (method and parameters, salt aside); ties to the class seen first | right whenever every entry is alike; on a mixed system, the fewest accounts stand out |
| D | Burn at the costliest class present | never faster than a real check, but a cheaper account is then *slower* to refuse when it does not exist than to reject when it does, so it stands out the other way |

### Why C

On a uniform system B, C and D agree, and all three close the leak A leaves
open. On a mixed system no single burn can match every account. C minimises
the number of accounts whose check differs from the burn. D only moves the
difference to the other side, and B depends on which account happens to come
first in the file.

The imitation is exact rather than approximate. The burn hashes the typed
password under the chosen entry itself, used as the setting, which is
precisely the work a real check against that entry does. So parameters
(`rounds=`, yescrypt's `j9T`, bcrypt's cost) are matched without being
parsed into a cost model.

### Details

- **Where:** `authlib::CostProfile`. `Authenticator::authenticate` reads the
  database once per call and takes both the profile and the user's entry
  from that one read, for every user. So the work before the hash does not
  depend on whether the account exists.
- **A locked account** (`!` in front of a hash, or `locked: true`) burns
  with its *own* hash, which is what checking it would cost if it were
  unlocked.
- **No entry that verifies:** the method new passwords get
  (`userdb::PASSWORD_METHOD`, now public so `authlib` follows it rather than
  a copy). A test fails if that method stops accepting the stand-in's salt.
- **`login`** reads the profile before looking the account up. For an
  unknown name it now burns after reading the password. Until today it
  computed no hash at all there: it read the password and refused at once.
- **`check_stored`**, the pure function `passwd` and `newgrp` use for
  their own accounts and groups, keeps the new-password cost. Its callers
  answer about an account the asker already knows exists, so there is no
  existence to leak.

### Revisit when

New passwords move off `$6$` (the request's item 2, `userdb::PASSWORD_METHOD`).
Nothing here then needs changing, but the stand-in's salt must suit the new
method, and the test above will say if it does not.
