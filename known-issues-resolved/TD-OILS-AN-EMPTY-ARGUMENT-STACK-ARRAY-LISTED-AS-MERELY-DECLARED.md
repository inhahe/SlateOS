### TD-OILS-AN-EMPTY-ARGUMENT-STACK-ARRAY-LISTED-AS-MERELY-DECLARED. `declare -p BASH_ARGV` printed the bare name where bash prints `=()` — 2026-08-11 — FIXED 2026-08-11

**Where:** `userspace/oils/src/interp.rs`, [`Shell::refresh_bash_arg_arrays`].
It materialised `BASH_ARGC`/`BASH_ARGV` into [`Shell::arrays`] without marking
them in [`Shell::array_valued`] — bash's `invisible_p`, the flag that separates
a name that was *assigned* from one that was merely *declared*.

```sh
$ shopt -s extdebug; f() { g() { declare -p BASH_ARGV; }; g; }; f
bash: declare -a BASH_ARGV=()
osh : declare -a BASH_ARGV
```

bash assigns both names rather than declaring them, so an empty one still
prints `=()`. It is the same distinction `declare -a q` and `q=()` draw, and it
is reachable here because the two stacks empty *independently*: a frame for a
call with no arguments puts a count on `BASH_ARGC` and nothing at all on
`BASH_ARGV`, so the pair can be a non-empty array beside an empty one.

**Fixed** in this commit: the flag is set with the arrays and cleared with them.

Corpus: `an-empty-argument-stack-array-is-assigned-not-merely-declared.sh`.
Unit test: `an_empty_argument_stack_array_is_assigned_not_merely_declared`.

Note the corpus turns `extdebug` on throughout. Without it osh populates
neither array, a separate and deliberate divergence documented under
TD-OILS-MISSING-SPECIAL-ARRAYS: bash exposes an undocumented base frame whose
behaviour contradicts its own man page ("The shell sets BASH_ARGC only when in
extended debugging mode").

**How it was found:** writing the corpus for
TD-OILS-AN-ELEMENT-WRITE-TO-A-MAINTAINED-NAME-SKIPS-ITS-SUBSCRIPTS-OWN-REFUSALS,
whose `BASH_ARGV[0]=9` row wanted to show that a refused element widens
nothing.
