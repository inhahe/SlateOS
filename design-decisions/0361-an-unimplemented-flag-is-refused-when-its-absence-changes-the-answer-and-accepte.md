## §361 — An unimplemented flag is refused when its absence changes the answer, and accepted as a no-op when it only omits an advisory

**Date:** 2026-08-22
**Decided by:** Claude (autonomous)
**Where it bites:** `userspace/coreutils/src/bin/bc.rs` first; the rule is
meant to apply to every utility in `userspace/coreutils/` that is missing a
flag its GNU counterpart has.

**In short:** Our `bc` does not implement three of GNU `bc`'s options. There
are only two things a program can do with an option it does not implement:
pretend it worked, or refuse to run. Pretending is friendlier and is what most
half-finished tools do — but for some flags pretending means printing a
confident wrong answer. The rule adopted here is to split the difference by
asking one question per flag: *if we ignore this, does the number that comes
out change?* If yes, refuse; if no, accept it and do nothing.

**The three flags, and which side each falls on.** All measured against GNU bc
1.07.1 through WSL:

| Flag | What GNU does with it | Ignoring it | Chosen |
|---|---|---|---|
| `-s` / `--standard` | non-standard constructs become errors: `echo 'print 1,2' \| bc -s` prints `(standard_in) 1: Error: print statement` and computes **nothing** | runs a program POSIX bc rejects and prints `12` | **refuse** |
| `-c` / `--compile` | emits dc code instead of evaluating | prints results where dc code was asked for | **refuse** |
| `-w` / `--warn` | adds `(Warning) …` on stderr; still prints `12` | loses an advisory; every value identical | **accept, no-op** |

**The alternatives considered.**

*Accept all three silently.* The friendliest, and wrong for `-s` in the way
this whole audit is about: a caller who wrote `bc -s script.bc` asked to be
told when their script leaves POSIX, and would be told nothing, forever, with
a `0` status. That is a silent wrong answer, which is the most expensive kind
of defect in this tree.

*Refuse all three.* Consistent and never misleading, but it breaks `bc -w`
scripts that work perfectly today and would work identically under our bc.
Refusing to run something we can run correctly is a real cost paid for
nothing but tidiness.

*The split, chosen.* The test is stated as a property of the *flag*, not of
our mood, so it is applicable by the next person without re-litigating: does
the flag's absence change a computed value or an exit status? `-s` and `-c`
do; `-w` does not. The cost is that the two treatments look inconsistent from
outside — `bc -w` runs and `bc -s` does not — which is why the reason is
written into `refuse`'s doc comment at the point of decision rather than only
here.

**Why not just implement them.** `-s` and `-w` need every AST production
tagged with whether POSIX bc has it, plus an off/warn/error mode threaded
through `Parser` — real work, unrelated to the command-line bug that surfaced
this, and tracked in `todo.txt` with the shape of the fix. `-c` needs a dc
emitter and is arguably not worth having at all. Refusing loudly is the
correct *interim*; it is not a decision to never implement them.

**A note on scope.** This is deliberately narrower than "always fail on
anything unimplemented". A flag that only affects diagnostics, formatting
hints, or performance can be a no-op; a flag that gates what is accepted, what
is emitted, or what is returned cannot. Where a flag is arguably both, refuse
— the cost of a spurious refusal is a visible error message, and the cost of a
spurious acceptance is a wrong answer nobody sees.
