## 600. A command that meets a word it does not understand stops, rather than pretending the word was not there

**Date:** 2026-08-25
**Decided by:** Claude (autonomous)
**Lane:** A

**In short:** nine shell commands used to silently throw away any argument
starting with `-` that they did not recognise. A misspelled flag therefore did
not fail — it ran a *different* command, successfully. The decision is that
every option parser in the shell now stops and names the word instead, even
though that makes some command lines which previously "worked" start failing.
The alternative — keep ignoring, since ignoring never crashed anything — is
what let `batch delete --dry-runn a b` delete `a` and `b` for real.

**The decision.** In `kernel/src/kshell.rs`, a word beginning with `-` is
either an option the command implements or an error. There is no third
outcome. A parser may not:

- end its `if`/`else if` chain without a final `else` that refuses;
- write `s => if !s.starts_with('-') { … }`, which discards the rest;
- fall back to a default when an option's *value* fails to parse;
- proceed when an option's value is missing entirely;
- filter `-`-leading words out of an operand list.

The last of those is why `--` had to land in the same change: once dash-leading
words are no longer quietly demoted to operands, a file literally named
`-empty` has no other way to be named, and our paths permit every byte but `/`
and NUL, so such files are real rather than theoretical.

**Alternatives, and why they lost.**

| | For | Against |
|---|---|---|
| **Keep ignoring** (status quo) | Never breaks a working line. Forward-compatible in the trivial sense: a script written for a newer shell still runs on an older one. | It runs it *wrongly*. The ignored word is nearly always a filter or a restriction, so ignoring it widens the search, enlarges the deletion set, or removes the containment that was asked for — and reports success. Forward compatibility bought by answering a different question is not compatibility. |
| **Warn and continue** | Names the mistake; keeps existing lines working. | A warning on a command whose output is being consumed by another command is not seen. `batch delete` would still have deleted the files, with a line of stderr nobody read. The status is the only channel a script can act on, and "continue" means the status says success. |
| **Refuse** (chosen) | The one channel a script can act on says what happened. A wrong answer reported as success is strictly worse than a missing one; this converts every instance of the former into the latter. | Breaks any existing line that passed an unknown-but-harmless flag. Also breaks the "pass the same flags to several tools, let each take what it knows" idiom — which nothing here does, and which is a bad idiom precisely because it hides typos. |

**Why the cost is small here specifically.** These are the kernel's own shell
builtins, used by the self-tests and by an operator at a serial console. There
is no installed base of scripts, and the commands that changed are the ones
whose flags *restrict* what they do, so anything that breaks was already doing
something other than what it said.

**What this does not settle.** Whether the rule should be mechanically
enforced. §296, §298 and §299 built checkers for the neighbouring rules (a
usage message must report failure; answering a query must report success), and
the same argument applies: a rule that lives only in reviewers' heads decays,
and this one was violated in nine places at once without anyone noticing. A
`scripts/check-option-refusal.py` would look for the three shapes above —
a predicate chain with no refusing `else`, a `starts_with('-')` catch-all that
does nothing, and `unwrap_or` on an option's value. It is not written. §299's
lesson applies to it in advance: key it on the *category* (does this branch
correspond to an argument the command did not understand?) rather than on the
literal spellings the nine happened to use, or it will pass the tenth.

**How to reverse.** Each refusal is a self-contained `else` arm or `let …
else`. Reverting the rule means restoring nine silent catch-alls, which is
visible in the diff and would have to be argued for one command at a time —
which is the right shape for a reversal of a rule like this.

**Where it came from.** `A-KSHELL-CUT-AND-FOLD-HAVE-NO-END-OF-OPTIONS-MARKER`
named two commands, because two is where the defect was *noticed*. Per §299,
the fix started with a survey of every option parser in the file instead, and
the survey found a strictly worse defect the report had not suspected. Full
write-up:
`known-issues.md` → `A-KSHELL-SEVEN-COMMANDS-DISCARD-AN-OPTION-THEY-DO-NOT-RECOGNISE`.
