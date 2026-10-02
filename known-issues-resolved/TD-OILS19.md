### TD-OILS19. `osh` alias expansion applies across `run_source` calls (input reads), not within a single parsed unit — 2026-08-03 — ✅ **RESOLVED 2026-08-03**

**Resolved on both counts, and a third found on the way.** The unit-at-a-time
half went away with `IncrementalParser` (built for `shopt -s extglob`, which has
the same "an option set by unit N governs unit N+1" shape): an alias defined on
one line now takes effect for the next, inside a sourced file, inside a function
body, and inside a `$( … )` body, while `alias x=…; x` on *one* line still does
not — all verified against the reference bash.

What was left was **where** an alias may be expanded. osh tested only the
previous token, which is not bash's rule: bash's `command_token_position` is a
*parser* question, so the answer depends on state the previous token alone does
not carry. Six divergences fell out of the difference, all now fixed by
`lexer.rs`'s `AliasOut`/`Prev` state (the running half of the test) beside
`CMD_INTRODUCERS` (the token half):

* A `)` ends a `case` arm's pattern, so the arm's **body** begins there —
  `case x in x) c;; esac` never expanded `c`.
* `;;`, `;&` and `;;&` end an arm, and what follows one is the next arm's
  **pattern**. osh expanded it; bash does not, which is exactly why bash's
  `command_token_position` excludes the three from an otherwise shared list.
* `time` puts a command after it, and so do the `-p`/`--` that belong to it
  (bash's `TIMEOPT`/`TIMEIGN`).
* An **assignment word** precedes the command word, so `x=1 c` expands `c`.
* A **leading redirection** does too, for as long as the command has been
  nothing but redirections (bash's `PST_REDIRLIST`): `>f c`, `2>&1 c`,
  `{v}>f c` and a leading here-document all expand, but `x=1 >f c` does not,
  because reading the assignment word ended the run.
* A reserved word only introduces a command where a reserved word was
  *acceptable*. osh had `echo if c` expanding `c` — `if` there is an ordinary
  argument, and bash prints `if c`.

Covered by `alias-expansion-happens-only-where-a-command-word-can-begin.sh` and
a lib test. The original text follows.

---


**Where:** `userspace/oils/src/interp.rs` (`run_source` → `parse_with_aliases`),
`userspace/oils/src/lexer.rs` (`expand_aliases`), `userspace/oils/src/main.rs`
(script/`-c`/REPL entry points).

**What:** `alias`/`unalias` are implemented and aliases are expanded over the
token stream *before parsing* (`parse_with_aliases`), matching bash's pre-parse
alias pass — including command-position-only expansion, the recursion guard
(`alias ls='ls -l'` terminates), and the trailing-blank rule (`alias sudo='sudo '`
makes the next word alias-eligible). However, because the interpreter parses an
entire `run_source` input in one shot, an alias **defined earlier in the same
input** does not take effect for **later commands in that same input** (e.g. a
script file or a single `osh -c '…'` string). Bash's own rule is close but not
identical: bash reads a script line-by-line, so `alias x=…; x` on one line does
not expand either, but `alias x=…` on line 1 and `x` on line 2 *does*. Our
REPL reads one line per `run_source` call, so interactive alias use behaves
correctly; only multi-line scripts / `-c` strings diverge (an alias defined on
an earlier line is not seen by a later line in the same file). Aliases inside
command-substitution bodies (`$(…)`) are also not expanded — those are parsed by
`parser.rs`'s recursive `parse(raw)`, which has no access to the shell's alias
table.

**Proper fix:** parse-and-execute the top-level input command-by-command (or
line-by-line) so the alias table is consulted incrementally, re-tokenizing/
re-expanding each command against the live `self.aliases` right before it runs;
and thread the alias table (or a `parse_with_aliases` variant) through the
command-substitution parse path. Deferred because it touches the top-level
execution driver; the current behavior covers interactive use and the common
"aliases defined in an rc file, used at the prompt" workflow.
