## 611. Section 607's "read the brackets" rule yields to a narrower one where a guessed operand would name a record rather than tune one

*Date: 2026-08-26*

**Decided by:** Claude (autonomous)
**Lane:** A

**In short:** Section 607 settled how the kernel shell decides whether a missing
word is an error or gets a default: read the command's own usage line, and if
the word is in `<angle brackets>` it is required, if it is in `[square
brackets]` it has a default. That rule is right for the ordinary case and stays
in force. This entry records the one class where it is wrong and gets
overridden: when the missing word is the *name or number of the thing being
written to*, rather than a knob on how the writing is done. Guessing a knob
gives you a wrong measurement. Guessing a name gives you a right-looking
measurement filed against the wrong object — or against an object that did not
exist until the guess invented it.

**The rule, in one sentence.** *An operand is required, whatever the usage line
says, when a wrong guess would silently select or create a different record
rather than change a value within the record the user named.*

Three findings produced it, in the order they were found.

**(a) The forged receipt.** Several accounting modules create a record on first
write — `send`, `record`, `write` and friends call an internal "get or insert"
so that the shell does not need a separate `create` step. That is convenient and
not itself a problem. It becomes one the moment the *key* has a default,
because then the command can be run with no key at all and will manufacture a
record for an id nobody supplied, then report success. The output is a receipt
for a transaction that did not happen. Sharpened after a second instance: what
makes a create-on-write path dangerous is not that it creates — it is that it
can be *reached with a key nobody supplied*.

**(b) An operand that says what a created object IS has no defensible default.**
For a `create` or `register` subcommand, every operand is a description of an
object that does not exist yet: a zram device's name, its disk size, a run
queue's cpu id. There is no prior state for a default to be consistent with, so
a default here does not fill a gap — it *asserts a fact about a device nobody
described*, and the assertion is then printed as though it had been measured.
So `create`/`register` arms take required operands regardless of their brackets.
(This generalises an earlier, narrower argument that a *denominator* cannot be
guessed because it silently rescales every ratio derived from it. The
denominator case is one instance of this; stating it as the denominator rule
alone would have missed a plain `name` operand, which is not a denominator and
is just as undefaultable.)

**(c) The complement: sequential ids make the safety net unreachable.** The
reflex defence of a defaulted id is that a wrong one hits nothing and returns
`NotFound`, so the user is told. That defence fails wherever ids are handed out
sequentially from zero, which is the norm in these modules: id 0 is not a
sentinel, it is the *first record ever created* and usually the longest-lived
and most interesting one. A defaulted id there needs no forged receipt to do
damage — it lands squarely on a real record, and the `NotFound` arm that was
supposed to catch the mistake is never reached.

**The alternative that was rejected.** Keep §607 absolute and fix these cases by
editing the usage line — move the operand from `[brackets]` to `<angles>` and
let the existing rule do the rest. That is tempting because it keeps one rule
instead of two, and it is what §607 was designed to allow. It was rejected
because it makes the *documentation* the thing under review rather than the
behaviour, and the two failure modes are not symmetric: a usage line that is
wrong about an optional knob costs a confused user, while a usage line that is
wrong about a record key costs a fabricated measurement that nothing downstream
can distinguish from a real one. A rule that catches the second kind should not
depend on someone having already written the brackets correctly. §607's rule
survives as the default; this one is the narrower, higher-priority exception,
and the two are applied in that order.

**What it is good for beyond deciding brackets.** Stated as (a)+(b) it is also a
*predictive screen* for unreachable mutators: a module that has a register-like
function and an empty `init_defaults` almost certainly has a shell arm that
create-on-writes past a defaulted key. Run against three modules before their
shell code was read, it named the defect in each — see known-issues.md,
`A-FS-MODULES-EXPOSE-MUTATORS-NOTHING-CAN-REACH`, whose whole population was
found this way.

**Where it bites.** `kernel/src/kshell.rs`, every `cmd_*` arm that takes a
device id, process id, cpu id or object name; and `scripts/check-option-refusal.py`,
which counts the remaining guessed operands but cannot itself tell a knob from
a key — that judgement is this entry.
