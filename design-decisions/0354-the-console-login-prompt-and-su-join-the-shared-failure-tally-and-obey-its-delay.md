## §354 — The console login prompt and `su` join the shared failure tally and obey its delay; `passwd` contributes but is never delayed

**Date:** 2026-08-21
**Decided by:** Operator ("i'll go with your recommendation", covering Q52, Q53,
Q54 and B-Q6 together; Claude's recommendation was option A with `su` joining)

**In short:** Getting a password wrong now makes you wait longer before the next
try — one second, then two, four, up to five minutes — and that count is shared
across programs, so failures at one prompt slow the others. The console `login:`
prompt and `su` were the last two left out, which meant the two prompts a human
actually types a password into were the two with no rate limit at all. They now
join. The known side effect, accepted deliberately: a program already running on
the machine can fail a password on purpose and thereby make the next person at
the keyboard wait, up to five minutes.

**Answers:** `open-questions.md` B-Q6 (deleted from that file by this entry).
**Related:** §347 (the tally is one shared fixed-slot file), §346 (an account
with no password may log in at the console but may not become root), §341
(`authlib`), `known-issues.md` →
`B-DOAS-COULD-NOT-VERIFY-ANY-PASSWORD-THE-SYSTEM-ACTUALLY-SETS` and
`B-PASSWD-VERIFIES-WITHOUT-AUTHLIB`.

### What changes

`login` and `su` read and write the shared tally, and honour its delay, for
every account **including root**. Before this, the console's only protection was
a per-process cap of three tries — after which `login` exited and was
immediately respawned, fresh. Guessing at the console was therefore not
rate-limited in any meaningful sense: fail three times, get a new prompt, repeat
forever. That is the same defect that was fixed for `doas`, in the more exposed
place.

`authlib`'s `Authenticator` gains a `rate_limited` / `note_failure` pair, so a
caller that owns its own verdict can still share the count. `login` needs this
because it owns the console's empty-password policy (§346) and therefore calls
the checking half of `authlib` directly rather than handing over the whole
decision.

### The cost, stated plainly

A program running as you can hold you at a delayed console prompt by failing
`doas` on purpose. Bounded at five minutes, never a lockout, always clears by
waiting — and the attacker is already running as you, which is the weakest
possible position from which to complain about being inconvenienced.

The sharper version, which `su` joining creates: `su` guesses at the *target's*
password, so a local user could hold **root** at a five-minute console delay
indefinitely, without ever having had root. That is a stranger's program
delaying the administrator rather than your own program delaying you, and it is
the real cost of this option.

It is accepted because the alternatives are worse in kind, not merely in degree:

| Option | Leaves unlimited guessing at | Why rejected |
|---|---|---|
| B — root exempt from the delay | the root account, at the console | Root is the highest-value account on the machine; exempting exactly it from the rate limit inverts the priority. `pam_faillock` has this switch (`even_deny_root`, default off) so it is mainstream, not absurd — but mainstream here is a compatibility default, not an argument. |
| C — console contributes but never obeys | the console, exactly as today | The justification is "a keyboard is human-speed", which stops being true the moment the console is a serial line or a VM monitor — both scriptable, both already supported. |
| D — leave it | the console *and* `su` | The current state, and the defect this whole change set exists to remove. |

A trades a bounded inconvenience for an unbounded exposure. Each of B, C and D
does the reverse.

### `passwd` stays outside the delay, and this is the decision for it too

B-Q6 noted that whatever is decided for `login` should be decided for `passwd`
in the same breath, since "the two prompts a human uses disagree about whether a
shared count applies" is precisely the inconsistency `authlib` exists to
prevent. So: `passwd`, when it asks for your *current* password before setting a
new one, **contributes to the tally but is never delayed by it**.

That is option C's shape applied to exactly one program, and the reason is
specific rather than a weakening of the rule. Changing your password is the
action you most want available at the moment you suspect it is compromised, and
a rate limit is precisely the mechanism that would take it away from you —
including from an attacker who could then *prevent* you locking them out by
holding your account at a delay. Every other prompt gates access to something;
`passwd` gates the remedy.

It still contributes, so guesses made there are not free elsewhere, and
`B-PASSWD-VERIFIES-WITHOUT-AUTHLIB` is closed by routing it through `authlib`
rather than by leaving it with its own verification.

**Revisit if:** the delay-your-neighbour effect turns out to be reachable in
practice rather than theoretically — e.g. anything in the desktop session fails
an authentication on a timer. The fix then is not option C but making that
program stop, since a program that routinely fails passwords is a bug on its own
terms.
