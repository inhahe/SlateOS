## §910 — An empty operand is an operand: kshell follows the reference tools rather than refusing what it cannot previously see

**Date:** 2026-09-04. **Decided by:** Claude (autonomous). **Lane:** A.

**In short:** the kernel shell used to throw away an argument written as `''`
(two quote marks with nothing between them — the empty string). Bash passes that
along as a real argument, so `echo a '' b` gives three arguments and not two, and
TD-KSHELL (b′) fixed the splitter to agree. That change made four commands
suddenly see an input they had never received before: `sed ''`, `awk ''`,
`tr a ''` and `column -s ''`. Somebody had to decide what each one means. The
decision was to look up what GNU and util-linux actually do and copy it, rather
than refuse all four on the grounds that an empty argument is probably a mistake.

### The alternatives

**(a) Refuse every empty operand.** An empty filename, separator or program is
very often a shell variable that expanded to nothing — `awk "$prog" f` with
`prog` unset — and refusing it turns a silent no-op into a loud complaint at the
exact moment the user's script has gone wrong. Two of the four already had a
refusal in place (`awk`'s `program.trim().is_empty()`, `column`'s empty-set fold),
so this was also the smaller change.

**(b) Match the reference implementation for each, case by case.** More work,
four separate answers, and two of them (`sed ''`, `awk ''`) mean *quietly do
nothing* — the outcome (a) exists to avoid.

### What was chosen, and why

(b). Three reasons, in order of weight.

The first is that (a) is not one policy but four guesses wearing one coat.
"An empty operand is a mistake" sounds uniform, but the four operands are not the
same kind of thing: an empty *program* has an obvious meaning (no rules, so do
nothing), an empty *separator set* has an obvious meaning (nothing separates, so
one field), and an empty *SET2* genuinely has none — there is no character to
translate to. GNU's answers are not arbitrary; they are what falls out of asking
what the operand means, and they disagree with each other precisely where the
meanings disagree. `tr a ''` is the only one of the four that GNU refuses, and it
is the only one where refusing is the answer to a question rather than a policy
about empty strings.

The second is that this shell exists to be a working shell, and a script that
runs under bash is the only usable definition of "working". A divergence that
only shows up on an edge case is worse than one that shows up immediately,
because it is discovered later and by someone with less context.

The third is that the "protects against an unset variable" argument for (a) is
weaker than it looks: it is exactly as true of bash, which does not do it. A user
who wants that protection has `set -u`, which is the tool for the job and applies
to every command at once rather than to the four that happen to have been
audited today.

### What it cost

`awk`'s and `column`'s existing refusals were removed. Both were dead code — the
splitter could not produce an empty word, so neither had ever fired — so nothing
that worked stopped working. `tr` gained a refusal it did not have
(`TrParseError::EmptySet2`), and that one is load-bearing: without it the padding
rule maps every member of SET1 to itself and `tr a ''` copies its input out and
exits 0.

`sed` needed a second change to make its answer reachable at all:
`parse_sed_command("")` falls through to `Unknown`, so an empty script had to be
skipped in `parse_sed_scripts` rather than parsed. Without that, passing the
empty word through would have moved `sed ''` from one exit-1 to a different
exit-1 — which is why the two land in the same commit.

### Where it bites

`kernel/src/kshell.rs`: `split_words`, `parse_sed_scripts`, `parse_awk_args`,
`parse_tr_args`, `column_parse_args`. Self-test rungs 42, 45, 53, 63 and 100
pin the four answers, and the `split_words` unit assertions pin the arity they
all rest on.

Not settled here: `cut`, `fold` and `base64` still pass an empty *filename*
through to `resolve_path`, which turns it into `/`. That is a different defect
with a much wider blast radius — see
`TD-A-AN-ABSENT-OPERAND-AND-AN-EMPTY-ONE-ARE-THE-SAME-STRING-IN-KSHELL`.
