## TD-B-IDENTITY-FROM-THE-ENVIRONMENT-A-FAMILY-NOT-AN-INCIDENT (lane B, 2026-09-11) -- CLOSED 2026-09-11

**CLOSED. The population is zero.** Every read of an identity variable is
gone from `userspace/`, `posix/`, `services/` and `init/`, and
`scripts/check-env-identity.py` (pre-push gate 23) holds it there with an
EMPTY baseline -- so it refuses any new one outright rather than comparing
against a list of permitted sites.

**Twelve programs, and they were not all the same thing.** Recording the
split matters more than the count, because treating them alike is how a
real finding gets buried in a list of tidy-ups:

| Severity | Programs | Why |
|---|---|---|
| Escalation | `sudo` (`$USER` keyed the credential cache -- no password), `login` (`$TTY` defaulted to `console`, passing securetty) | a caller-set value defeated an access control |
| Cross-user action | `screen` (attach to another user's sessions), `lp` (`lprm` cancelled another user's job), `mesg`/`wall` (sender spoofing, one to a terminal and one to every terminal) | the value chose WHOSE thing was acted on |
| Broken outright | `at` (the fallback was a PID, so the daemon refused every job it had accepted) | not a security bug at all |
| Misleading default | `logger`, `firejail`, `ssh`, `chroot`, `mktemp` | the read was legitimate or harmless; the FALLBACK named `root` |

**The fallback was its own defect, separate from the read.** Five of the
twelve fell back to the literal `root` for a caller the build could not
identify -- in the system log, on every sandbox, as the remote login name
`ssh` reached for. An unidentifiable caller is not root; it is
unidentifiable. `firejail`'s test still records an earlier repair of the
same fallback whose original comment read "Should return at least
\"root\" as fallback" -- the defect pinned as the contract.

**What made it findable:** every one had a correct implementation beside
it. `sudo`'s `effective_uid` already read `getuid(2)` under a note saying
"The fallback was the caller's to set" while the username two lines away
read `$USER`. `wall`'s `get_tty` read the real descriptor while the name
in the same banner came from the environment. `mktemp`'s environment step
sat between two correct answers.


**In short:** Four times now, a program has taken a value that decides an
authorization outcome from an environment variable the caller sets. Three were
found and fixed on 2026-09-11; the fourth was found and fixed weeks earlier and
is why the other three were looked for at all.

| Program | Value | What it decided | Fixed |
|---|---|---|---|
| `sudo` | `$UID` | the caller's uid | earlier — the fix's note is what named the pattern |
| `sudo` | `$HOSTNAME` | which sudoers rules apply (host half) | 2026-09-11 |
| `sudo` | `$USER` | which rules apply, whose password, **which credential cache** | 2026-09-11 |
| `login` | `$TTY` | whether root may log in at all (`/etc/securetty`) | 2026-09-11 |
| `at` | `$USER` | the submitter a job is filed under, compared before it runs | 2026-09-11 |

**Why it is a family and not four accidents.** Every one of them had a correct
implementation sitting beside it. `sudo`'s `effective_uid` already read
`getuid(2)` and carried a note saying "The fallback was the caller's to set" —
and the username and hostname two lines away still read the environment.
`login`'s `check_securetty` was correct in itself; only its input was wrong.
The defect is never in the checking code, which is where anybody looks.

**The two that were escalation, not mis-attribution:**

* `sudo` `$USER` keyed the credential cache, so `USER=alice sudo` found alice's
  live timestamp, **asked for no password**, and ran under alice's rules.
* `login` `$TTY` defaulted to `"console"` when unset — the name a securetty
  file is likeliest to list — so the control passed by default, on a terminal
  nobody had looked at, with the caller doing nothing at all.

**What was checked and is clean:** `su` and `newgrp` read only `$TERM` and
`$SHELL`, which are legitimately the caller's. Of 22 userspace crates that read
an identity variable, the rest use `$HOME` for a config path or `$SHELL` for
which shell to launch — what those variables are for.

**Still open:** `sudo`'s `current_tty` for the audit log, recorded separately
below. It is the same shape with a bounded consequence.

**The fifth was found by looking, not by accident** (`at`, 2026-09-11), which
is the first time that has happened with this family. It also carried a second
defect with no security framing at all: the fallback was
`format!("uid{}", process::id())` — the PROCESS ID, so a job queued by pid 4123
was filed under `uid4123` and the daemon calling itself `uid5678` refused it.
With `$USER` unset, which is how a daemon started by init runs, `at` refused
every job it had accepted. `crontab` had the identical pair and its repair note
is what made this one recognisable.

**The gate this needs, now that there is enough data to design it.** Taint
analysis is the obvious approach and the wrong one — the value runs through
several functions and the checkers here are regex-based. The tractable rule is
a RATCHET over the read itself:

> Reading `USER`, `LOGNAME`, `UID`, `EUID`, `GID`, `HOSTNAME`, `TTY`,
> `USERNAME` or `SUDO_USER` from the environment is a finding unless the site
> is in the baseline.

The legitimate uses are few and reviewable — 22 crates read one of these, and
most are `$HOME` for a config path or `$SHELL` for which shell to launch — so
the baseline starts small and only shrinks. It does not need to prove the value
reaches an authorization decision, which is the part that cannot be done with
these tools; it makes each new read say why it is not one of these five.

Two rules considered and rejected: keying on crates that depend on `authlib` or
`userdb` would have missed `at`, which depended on neither until it was fixed;
and listing the security-relevant crates by name is an enumeration that misses
the next crate by construction, which is the defect shape this tree keeps
finding.
