## B-PASSWD-VERIFIES-WITHOUT-AUTHLIB — 2026-08-21 — **FIXED 2026-08-21**, see Resolution at the end

**In short:** every program on this system that asks "is this your password?"
routes through one of two shared verifiers, which count failed attempts and
slow an attacker down. One program does not: `passwd`, when it asks for your
*current* password before letting you set a new one. It gets the answer right —
it uses the same underlying comparison as everything else — but it does the
counting-and-slowing part not at all, so guesses against that one prompt are
free and unlimited. Whether that should change is a genuine tradeoff, written
out below.

**Where:** `userspace/passwd/src/main.rs:651`, the `verify_password(&old_pw,
&entry.hash)` call behind the `Current password:` prompt. `verify_password`
(line 336) is a one-line wrapper over `posix::crypt::verify`.

**How it got this way — not by decision.** `passwd` was moved onto
`posix::crypt::verify` on 2026-08-17, closing
`requests/c-b-passwd-and-login-disagree-about-etc-shadow.md`. `authlib` did not
exist yet; it was built later, after `design-decisions.md` §329 and §341 found
that several programs were each answering the password question with their own
arithmetic. Programs written or repaired after that point adopted `authlib`.
`passwd` was already correct by the standard of its own day, so nothing ever
sent anyone back to it. It is debt by omission, not by choice — which is the
only reason it is filed rather than simply fixed: the fix has a real cost.

**The tradeoff.**

| | Leave it | Route it through `authlib` |
|---|---|---|
| Guessing at the `Current password:` prompt | unlimited and free | shares the per-user tally with `login`, `su`, `doas` |
| Someone mistypes at a `doas` prompt three times | your own `passwd` still works | your own `passwd` prompt is now delayed too |
| Locked account (`!` prefix) | already refused outright at line 623, before any prompt | `authlib` returns `Locked`, which agrees — nothing to special-case |

**A dead condition found while writing this — FIXED 2026-08-21, and it was
load-bearing in the wrong direction.** The old-password gate read
`if !entry.hash.is_empty() && !entry.is_locked()`. The second conjunct could
never be false, because line 623 has already returned `1` for every locked
account. Dead code, so removing it changes nothing today — but which way it was
dead matters:

| If someone later removes the line-623 guard | Before | After |
|---|---|---|
| locked account reaches the old-password gate | `is_locked()` is true, so the whole gate is skipped — **no current password is ever asked for, and the change proceeds** | gate is entered on `!hash.is_empty()` alone; the stored `!$6$…` has no recomputable method, so `crypt::verify` returns false and the change is **refused** |

So the redundant conjunct was not merely noise: it was a second, silent
implementation of the locking policy that failed *open* if the first one were
ever touched. It now fails closed. This is the general shape of the thing —
a guard duplicated in two places is not twice as safe, it is one guard plus one
place for the policy to disagree with itself.

The argument for leaving it is not weak. `authlib`'s tally is per *user*, not
per program, so folding `passwd` in means anyone who can reach any prompt as
you — a `doas` prompt in a shell you left open, a lock screen — can also stop
you changing your password by failing at it. A password change is the one
action you most want available to a user who suspects their password is
compromised, and rate-limiting is the one mechanism that makes it unavailable.

The argument against is that this is the only prompt in the system where an
attacker who already has your shell can guess at your password without cost or
trace, and "the fix would be annoying" is how every uncounted prompt stays
uncounted.

**What the proper fix looks like** (if the answer is "route it"): `authlib`
gains a way to verify *without* consuming an attempt from the shared tally
while still recording to the audit log — the distinction being whether a
failure should impede a later, different program. That is a change to
`authlib`'s contract, so it wants doing at the same time as the on-disk tally
described under `B-DOAS-COULD-NOT-VERIFY-ANY-PASSWORD-THE-SYSTEM-ACTUALLY-SETS`
→ "Cross-invocation rate limiting", not separately.

**Update 2026-08-21 — the tally now exists, and this question folded into a
larger one.** The on-disk shared tally landed the same day (see the entry
above). That did not settle this; it sharpened it. The counter-argument in the
paragraphs below — "anyone who can reach any prompt as you can stop you
changing your password" — was hypothetical while the tally was per-process and
is concrete now that it is per-user and persistent. It is also the *same*
question, in a different prompt, as whether `login` should obey the shared
tally. Both are queued together as `open-questions.md` → **B-Q6**; answer that
and this follows from it. Do not decide this one in isolation — the two prompts
disagreeing about whether a shared count applies to them is precisely the
inconsistency `authlib` exists to prevent.

**Not a security hole today, and worth being precise about why:** reaching this
prompt requires already running as the account whose password is being changed.
An attacker there can read that account's files and act as it. What the missing
rate limit costs is the ability to *learn the password itself* — which matters
because users reuse passwords, and because knowing it converts shell access
into the ability to pass a `doas` prompt. So: real, bounded, not urgent.

**Found by:** the call-site scan described in the postscript to
`requests/c-b-passwd-and-login-disagree-about-etc-shadow.md` —
`grep -rn 'crypt::verify' posix/src userspace/*/src services init`, which
returns exactly three production call sites and requires each to be justified.

### Resolution — 2026-08-21, and the tradeoff was split rather than picked

B-Q6 was answered as `design-decisions.md` **§354**, and this followed from it
exactly as the entry above said it would. The answer to the table's tradeoff is
**neither column**: `passwd` **contributes to the tally and is never delayed by
it**. Guessing at `Current password:` is no longer free — it costs the guesser
time at `login`, `su` and `doas` afterwards — but no delay earned anywhere can
stop you changing your password. That is the whole of the counter-argument the
entry raised ("a password change is the one action you most want available to a
user who suspects their password is compromised") granted in full, at no cost
to the thing the rate limit was for.

What changed in `userspace/passwd/src/main.rs`:

| Before | After |
|---|---|
| `verify_password` → `posix::crypt::verify` | the prompt asks `authlib::check_stored`, the same function every other program asks |
| a second `posix::crypt::stored_method` probe to tell "unverifiable" from "wrong" | `Outcome::needs_administrator()` — the classification comes back *with* the verdict |
| nothing counted | `judge_old_password` does the bookkeeping, and is a plain function so the policy is testable without a terminal |
| — | a verified current password **clears** the run of failures, as a successful login does |

Three failures are deliberately *not* counted, each for its own reason:

- **An unverifiable entry** (`Outcome::Unusable`). No answer can ever match it,
  so a wrong one teaches an attacker nothing and taxing it would lock a user
  out of `login` for an administrator's mistake.
- **A locked account.** Refused before the prompt exists, so nothing was typed.
- **Root changing another user's password.** Skips the old-password check
  entirely; there is no guess to charge.

`verify_password` is gone from production code and now lives in the test
module, because it had stopped being the program's verifier and a production
function kept alive only for its tests is the same duplicate-policy problem in
a quieter form. What it asserts there is the property that *is* this crate's to
keep, and the one
`requests/c-b-passwd-and-login-disagree-about-etc-shadow.md` was filed over: a
password this program **writes** is one the system's verifier **accepts**.

**Tests:** `cargo test -p passwd` — 73 passed, up from 69. The four new ones:
`a_wrong_current_password_is_charged_to_the_shared_tally`,
`passwd_is_never_delayed_by_the_tally_it_contributes_to`,
`an_unverifiable_entry_is_reported_not_counted`,
`verification_agrees_with_authlib_for_every_shape_of_entry`. Each uses
`Authenticator::with_stores`, which counts in memory and attaches no faillock
file — a suite that used `Authenticator::new()` would run up a real delay
against a real account on the developer's machine every time it ran.
