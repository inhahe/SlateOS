### TD-OILS-HELP-DECLARE-SUMMARY-LINE-IS-NOT-BASHS. `help declare` says "Declare variables and give them attributes." where bash says "Set variable values and attributes." — 2026-08-06 — ✅ FIXED 2026-08-06

**Where:** `userspace/oils/src/interp.rs` — the `help` builtin's text table.

**What.**

```text
$ help declare | head -3
bash: declare: declare [-aAfFgiIlnrtux] [name[=value] ...] or declare -p …
      Set variable values and attributes.
      <blank line, indented by four spaces>
osh : declare: declare [-aAfFgiIlnrtux] [name[=value] ...] or declare -p …
      Declare variables and give them attributes.
```

Two differences: the summary sentence, and bash's blank-but-indented line after
it (four spaces, then nothing) which osh drops. "Declare variables and give them
attributes" is bash's line for `typeset`, not `declare` — the two were
transposed. Noticed while probing `declare -G`; no corpus case exercises `help`
for these builtins yet.

**Fixed.** The whole table was diffed against bash 5.2.37 rather than just this
entry: `compgen -b` gives the same 61 names on both sides, and walking `help -d`
over them found **28** wrong descriptions, not one. They fell into two groups —
a handful that were genuinely transposed (`declare`/`typeset`, `.`/`source`) and
a long tail that had been paraphrased into more modern-sounding English than
bash's own (`dirs - Display directory stack.`, `printf - Formats and prints
ARGUMENTS under control of the FORMAT.`, `unalias - Remove each NAME from the
list of defined aliases.`). All 28 are now bash's text verbatim; the
reserved-word topics (`for ((`, `{ ... }`, `(( ... ))`, `variables`, …) already
matched. New corpus case
`help-says-of-each-builtin-what-bash-says-of-it.sh` walks `help -d` and `help -s`
over every name `compgen -b` yields, plus the keyword topics, a pattern match
and a miss.

The blank-indented line this entry also noticed is a different, larger gap; see
TD-OILS-HELP-HAS-NO-LONG-DESCRIPTIONS below.
