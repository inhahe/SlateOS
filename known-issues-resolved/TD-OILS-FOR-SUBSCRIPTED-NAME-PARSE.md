### TD-OILS-FOR-SUBSCRIPTED-NAME-PARSE. `for 'a[0]' in …` is a parse error in `osh` and a runtime complaint in bash — 2026-08-03 — ✅ FIXED 2026-08-04

**Where:** `userspace/oils/src/parser.rs` — the `for` name production.

**What:** bash accepts any word where a `for` loop's control variable goes and
complains at *run* time if it is not an identifier:

```sh
$ bash -c "for 'a[0]' in x; do :; done; echo rc=\$?"
bash: line 0: `'a[0]'': not a valid identifier
# rc=1, and the rest of the *unit* still runs

$ osh -c "for 'a[0]' in x; do :; done"
osh: line 1: syntax error near unexpected token `'a[0]''
```

The difference is not just the wording: a parse error abandons more input than
bash's runtime refusal does, and it is raised even when the loop would never
have run. `select` had the same shape, and osh got the *bare* invalid name
(`for 1x`) wrong too — it was a parse error there as well, where bash's is a
run-time failure like any other.

**Fixed 2026-08-04.** The parse-time check is gone; the productions now accept
any word and store its source spelling.

- `ForClause::var` / `SelectClause::var` are `Str` (the word as *written*)
  rather than a validated `String`. This mirrors `FunctionDef::name`, which
  bash defers in exactly the same way and for the same reason.
- `Shell::loop_var_name` decides the question at run time, and
  `Shell::exec_for` / `exec_select` ask it before doing anything else — which
  matters, because bash refuses the name without expanding the `in` list at
  all (`for 'a[0]' in $(cmd)` never runs `cmd`) and without printing a
  `set -x` header.
- The test is on the spelling, never on the expansion: `"x"` is refused though
  a bare `x` is fine, and `$v` is refused without reading `$v`. Storing the
  spelling is what makes that possible and is also what gets quoted back.
- `wrap_parse_message` no longer passes `` `WORD': not a valid identifier ``
  through untagged, because nothing raises it from the parser any more. bash
  never did either: every name it complains about — a loop variable, a
  function name, a builtin's operand — is checked while the command runs.

Three things measured on the way that were not in the original note:

- **posix mode makes it fatal — for `for` only.** `set -o posix` turns the
  refusal into an unconditional unwind with status 2, which `!`, an `if`
  condition, `eval` and an ERR trap all fail to spare (a subshell contains it,
  and the EXIT trap still runs) — the same shape as a bad function name.
  `select` keeps status 1 and carries on even in posix mode, because bash wrote
  that branch into `execute_for_command` and nowhere else.
- **A reserved word is a fine loop variable.** `for do in one two` and
  `for in in one` both work: bash only promotes a word to a reserved word in
  command position, and this slot is not one.
- **It arms neither `errexit` nor the ERR trap** — though `$?` is 1 and
  `&&`/`||`/`if` all see the failure. The boundary is the enclosing *simple
  command*, which is what `Shell::for_name_failed` records:

  ```sh
  set -e; for 'a[0]' in x; do :; done;      echo AFTER   # AFTER (exempt)
  set -e; { for 'a[0]' in x; do :; done; }; echo AFTER   # AFTER (a group is
                                                        #  not a command)
  set -e; while :; do for 'a[0]' in x; do :; done; break; done  # nor is a
                                                        #  loop body
  set -e; f() { for 'a[0]' in x; do :; done; }; f       # exits: `f` failed
  set -e; eval 'for "a[0]" in x; do :; done'            # exits: so did eval
  ```

  The flag is armed at the refusal, read by `exec_and_or`, and cleared *after*
  every simple command rather than on entry to one — the entry-clear the
  neighbouring `special_builtin_failed` uses would not do, since this flag is
  set by a compound nested *inside* the simple command whose status is being
  judged. A subshell clone starts it `false`: the parent judges a subshell by
  its exit status alone.

**Known deviation inherited here:** the spelling is produced by
`Parser::token_display`, so a `$'…'` name is quoted back as `$'abc'` where bash
says `'abc'` — the lexer has already decoded it. That is
TD-OILS-ANSIC-ERROR-SPELLING, which this shares with every other diagnostic
that names a word.

**Pinned by** `tests/corpus/a-for-loops-variable-is-a-word-until-the-loop-runs.sh`,
the `loop_variable_is_any_word` unit test in `src/parser.rs` and
`unexpected_token_and_identifier_errors_match_bash` in `src/interp.rs`.
