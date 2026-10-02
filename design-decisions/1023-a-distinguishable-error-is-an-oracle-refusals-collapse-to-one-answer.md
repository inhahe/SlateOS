## 1023. A distinguishable error is an oracle: refusals collapse to one answer

**Date:** 2026-09-15
**Decided by:** Claude (autonomous)
**Lane:** B

**In short:** When `loginctl` asks about a user account that belongs to
somebody else, logind answers "no such user" -- the same words it uses for an
account that has never existed. That is deliberate, and it is a small lie. The
alternative, "you are not allowed to see that account", is more helpful and
also tells an attacker, one guess at a time, exactly which accounts exist on
the machine. We chose the less helpful answer for every refusal that a caller
could ask over and over.

**The decision.** Every method keyed on a user id -- `GetUser`,
`TerminateUser`, `KillUser` -- gives `NoSuchUser` for all four of: a uid that
is not a number, a uid nobody has ever used, a uid that exists and belongs to
somebody else, and a uid that exists and is the caller's own but has no state
to report. `authorize_uid` in `userspace/logind/src/bus.rs` is the single
place that decides it. `authorize` had already made the same call for session
ids and says so in its own comment; this generalises it and names why.

**Why it is a real tradeoff and not an obvious win.** The refused caller is
usually not an attacker. They are an administrator who typed the wrong uid,
and we have just sent them to check their typing when the actual problem was
their privileges. That cost is paid on every honest mistake, forever, to deny
an attacker a capability they can get other ways on most systems anyway. It is
paid in the place where diagnostics matter most -- an operator at a terminal
with something broken.

We took it because the asymmetry is unequal in a way that does not show up in
either anecdote. The administrator's confusion is recoverable in seconds by
trying as root. The enumeration is not recoverable at all: it is silent,
unlogged, automatable, and its product -- a list of who uses this machine --
is durable long after the hole is closed.

**The rule, stated so it transfers.** *If a caller can ask a question
repeatedly and the answer varies with something they are not entitled to know,
the variation is the leak -- not the answer.* An error that distinguishes
"absent" from "forbidden" is a one-bit read of privileged state with
unlimited retries. This is why the fix is never "make the message vaguer"; a
vague message that still differs between the two cases leaks exactly as much.
What must collapse is the *distinction*, and vagueness is only the side effect
of collapsing it.

**Where it does NOT apply, which is most places.** `ERR_UNKNOWN_CALLER` is
reported as itself, because "I could not identify this connection" is a fact
about the connection rather than about any user -- the caller learns nothing
about anyone else by receiving it. `ERR_NO_SESSIONS` likewise survives as a
distinct answer for root, who is already entitled to enumerate: collapsing it
there would cost the diagnostic and buy nothing. And `KillUser`'s four
refusals (bad arity, unparsable signal, leader pid 0, no sessions) stay four
different answers, because they describe the *request* rather than the
machine's population.

The discriminator: collapse errors that vary with **state the caller may not
read**; keep errors that vary with **what the caller sent**.

**How it is held.** `acting_on_another_uid_is_indistinguishable_from_acting_on_nobody`
asserts the four cases are one `Reply`, and drives both new methods through it
rather than testing the shared helper once -- a change that re-inlined the
check into one method would otherwise leave the other's test green. Sabotaged
before being believed: making the someone-else case answer `AccessDenied`
fails exactly two assertions.

**Arrived at twice, independently.** Lane C reached the same conclusion from
the other end the same day, reviewing an unrelated settings page, and put it
this way: a distinguishable error is an oracle. Two lanes finding one rule
from different evidence is the reason it is written here rather than left in
the comment where it started -- the next person to read those two error
branches will see one is more helpful than the other, be right about the code,
and wrong about the consequence.
