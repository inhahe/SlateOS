## B-DOAS-COULD-NOT-VERIFY-ANY-PASSWORD-THE-SYSTEM-ACTUALLY-SETS — 2026-08-21 — FIXED

**In short:** `doas` is the program that lets an ordinary user run one command
as root after typing their password — SlateOS's equivalent of `sudo`. It
checked that password with arithmetic of its own that no other program in the
system used, so a password set with `passwd` could never open a `doas` prompt.
Every attempt printed "authentication failed", which reads to the person typing
as a forgotten password rather than as a broken program.

Found by asking, after `B-THE-SSH-STACK-AUTHENTICATED-NOBODY`, which *other*
programs in this tree still answer "is this the user's password?" themselves.
`doas` was the only one left, and it was the one guarding root.

### What it did

`doas` understood exactly one stored format:

```text
$sha256$<salt>$<hex digest of sha256(salt || "$" || password)>
```

and returned `false` for anything else. `passwd` writes `$6$` SHA-crypt through
`posix::crypt`. The two had nothing in common. The section header above the
function read `Password hashing / verification (matches passwd utility
format)` — a claim that was true when it was written, stopped being true when
`passwd` was centralised (`design-decisions.md` §329), and was checked by
nothing, because the tests hashed with `hash_password` and verified with
`verify_password` and so agreed with themselves no matter what the format was.

Three consequences, in descending order of how bad they were:

| | |
|---|---|
| **`doas` did not work at all** | Not "worked weakly" — a correctly-typed password was refused, always. The only accounts that could escalate were ones covered by a `nopass` rule, i.e. the ones that never type a password |
| **`/etc/users.yaml` was invisible to it** | It read `/etc/shadow` directly, so a user in the native database had no entry `doas` would even look for. Same message |
| **No distinction between a typo and an entry nothing can recompute** | Both printed "authentication failed". The second needs an administrator and will never clear on its own; the first clears on the next attempt |

### Why this is filed as a security defect and not a bug

It failed *closed*, so nobody ever got root they should not have had. The
danger is the second-order one: a privilege gate that no legitimate user can
pass does not stay in place. It gets a `permit nopass` rule written around it,
or it gets removed, and either way the arithmetic that was supposed to protect
root is gone — this time on purpose, and with a comment explaining that `doas`
"doesn't work". That is a worse position than the bug, and it is where this was
heading.

It is also the fifth instance of one root cause. §329 found three programs
disagreeing about `/etc/shadow` and centralised them into `posix::crypt`; §341
added `authlib` on top; `sshd` turned out to have a fourth copy while `authlib`
was being written; `doas` is the fifth. Centralising does nothing for a program
that never looks, and nothing in the build fails when one does not.

### The fix

`doas` now calls `authlib::Authenticator::authenticate`, which consults
`/etc/users.yaml` then `/etc/shadow`, hands the stored entry to `crypt` as its
own *setting* rather than taking it apart to find a salt, and reports `Locked`,
`NoPassword` and `Unusable` separately from `Rejected`. `main` gates on
`is_accepted()`, so only `Accepted` admits anyone. The `sha2` dependency existed
solely for the private hasher and is gone.

Two deliberate choices:

- **The prompt now comes before the lookup.** The old order exited with a
  distinct message for "no shadow entry" and for "locked" *before* printing a
  prompt at all. Now every failure says the same thing, except that an entry
  nothing can recompute additionally says so — that one is a broken system,
  not a wrong password, and only an administrator can clear it.
- **An account with no password set is refused.** `login` resolves the same
  `authlib::Outcome::NoPassword` the *opposite* way, because a deliberately
  passwordless account at the machine's own keyboard is a long-standing Unix
  choice. Escalating to root from one is not the same statement, and reading it
  as consent would turn every passwordless account into a root shell.
  `nopass` in `/etc/doas.conf` is the only consent that counts here.

### Cross-invocation rate limiting — FIXED 2026-08-21

**Was:** `authlib`'s failure tally lived in the `Authenticator`, which for
`doas` lives for one invocation. `sshd` and `ftpd` keep one per daemon, so
their tallies outlive a connection; `doas` cannot, because it *is* the process.
So repeated `doas` attempts were not rate-limited relative to each other —
every invocation started the escalating delay again at zero — which is exactly
the shape of an attacker who already has a shell as the user and is guessing
toward root. The same held for `login` and `su`, and for `passwd`'s
`Current password:` prompt by way of a different gap (see
`B-PASSWD-VERIFIES-WITHOUT-AUTHLIB`).

**Now:** `authlib` keeps the tally on disk as well as in memory, in one shared
table at `authlib::DEFAULT_FAILLOCK` (`/var/run/authlib/tally`), so every
program that authenticates through `authlib::Authenticator` counts against
*one* tally per user. Today that is `doas`, `sshd`, `ftpd` and `logind` — but
not yet `login` or `su`, which is the remaining half of the gap and is written
up under "Still open" below. `Authenticator::new()` uses it; `with_stores`
stays memory-only, so a
test suite or a chroot cannot run up a real user's failures. Every call reads
the file fresh (a failure another program recorded a moment ago must count
against *this* attempt), takes the field-wise maximum of the in-memory and
on-disk rows, and writes the advanced count back to both. A write that fails is
ignored on purpose: the in-memory tally still limits the running process, so an
unwritable `/var/run` degrades to the old behaviour rather than refusing to
authenticate anyone.

The table's shape is `userspace/authlib/src/faillock.rs`, and three attacks
drove it — see `design-decisions.md` §347 for the alternatives:

| Attack | What stops it |
|---|---|
| A username from the login prompt is attacker-chosen text; a file *named* for it is a path-traversal and an unbounded-file-creation primitive | One fixed-size file, 1024 slots, usernames hex-encoded so no name can forge or corrupt a row |
| Probing which accounts exist by watching which ones get rate-limited | An invented username takes a slot exactly as a real one does; nothing distinguishes them |
| Flooding the table with invented names to evict the record of the account actually under attack | Eviction is by *fewest* failures, oldest first — the attacked account is evicted last |

**What did not change:** a refused (rate-limited) attempt is still not counted,
so an attacker cannot hold a real user out by refreshing their own refusal; and
the delay is still `delay_for` — doubling from 1s once `FREE_ATTEMPTS` are
spent, capped at `MAX_DELAY_SECS`. The state lives under `/var/run` rather than
`/var/lib` deliberately: it describes an attack in progress, not a durable fact
about the account, and a reboot is not something an attacker can arrange more
cheaply than waiting out five minutes.

**Tests:** `a_program_that_runs_once_inherits_the_previous_run_s_failures`
builds a fresh `Authenticator` per attempt to stand for a short-lived process
and asserts a brand-new one is refused *even when presenting the correct
password*; `a_success_in_one_program_clears_the_tally_for_the_next`;
`a_memory_only_verifier_writes_no_shared_file`;
`combining_two_tallies_takes_the_longer_delay_from_each_field`; plus twelve in
`faillock` covering injection, truncation, damaged rows, the slot cap, eviction
order and the temp-file-and-rename store.

### Still open — `login` and `su` do not share the tally

Both reach the right *verdict* through shared code, but neither goes through
`Authenticator`, so neither reads or writes the shared count:

| Program | What it calls | Why it is not `Authenticator` |
|---|---|---|
| `login` | `authlib::check_stored` (`userspace/login/src/main.rs:176`) | It owns one policy `authlib` deliberately declines to rule on: an account with an empty password field is entered by pressing Enter *at the machine's own keyboard*. `Authenticator::authenticate` reports that as `NoPassword` and leaves the caller to decide, so `login` calls the checking half directly and never touches the counting half. |
| `su` | `userdb::Record::check_password` | It predates `authlib` and reads `/etc/users.yaml` through `userdb` for other reasons anyway. |

The consequence is worth stating plainly: `login` still caps a *single process*
at `MAX_LOGIN_ATTEMPTS` and then exits, so the delay never escalates across the
getty respawn, and failures at the console do not slow down a subsequent `doas`
guess or vice versa. One tally per user is the point of the change, and there
are still three tallies.

The fix `login` wants is not "call `authenticate` instead" — that would take
the console's empty-password policy away from it. It is for `authlib` to expose
the counting half on its own, so a caller that owns its verdict can still share
the count: a `rate_limited(user) -> Option<retry_after>` to consult before
prompting and a `note_failure(user)` after, with the existing `reset` for
success, and `authenticate` refactored to be exactly those two around
`check_stored` so there is one implementation rather than two.

**That change has a tradeoff that should be decided, not assumed.** Once
`login` shares the tally, an unprivileged process running as the user can hold
that user at a delayed console prompt by failing `doas` on purpose — which is
`pam_faillock`'s behaviour on Linux too, and is bounded here by
`MAX_DELAY_SECS` (five minutes, never a permanent lockout). Whether five
minutes of console delay purchasable by any local process is the right price
for one tally per user is a judgement call — and if `su` joins at the same
time it is sharper still, because `su` guesses at the *target's* password, so
any local user could hold **root** at a delayed console prompt without ever
having had root. That is the operator's call, not mine: it is queued as
`open-questions.md` → **B-Q6**, with four options and a recommendation.
`design-decisions.md` §347 records it as the open half.
