### BUG-OILS-WHOLE-SCRIPT-PREPARSE. A script was parsed in one shot, so `shopt -s expand_aliases` never took effect and a syntax error un-ran earlier commands — 2026-07-27 — ✅ RESOLVED 2026-07-27

**Symptom.** Found by the differential harness with a new
`tests/corpus/command-type-alias.sh` case. Two divergences, one root cause:

```sh
shopt -s expand_aliases
alias hi='echo hello'
hi                 # bash: hello        osh: line 3: hi: command not found
```

```sh
echo hi
echo two )         # bash: prints "hi", *then* the syntax error
                   # osh:  prints nothing, only the syntax error
```

**Root cause.** `Shell::run_source_flow_out` parsed the *entire* input in one
`parse()` / `parse_with_aliases()` call and only then executed it. Both the alias
decision (`aliases_enabled()`) and the alias table were therefore sampled once,
**before the first command ran**, so a `shopt -s expand_aliases` or `alias …`
executed by the script could not possibly affect the parse of the rest of it — a
non-interactive script could never use aliases at all. The same up-front parse
made a syntax error anywhere in the file suppress every command in it.

bash instead runs a read → parse → execute loop whose unit is one **logical
line**. Measured against bash 5.2:

| probe | bash |
|---|---|
| `shopt -s expand_aliases` on line 1, `alias`/use on later lines | expands |
| `alias a=…; a` — define and use on the *same* line | **not** expanded |
| `echo hi` newline `echo two )` | `hi` prints, then the error |
| `echo one; echo two )` on one line | **nothing** runs — the whole line failed |

So the unit is neither the whole file nor a single command: it is everything up
to a terminating newline (which may hold several `;`/`&`-separated commands, and
may span physical lines when a compound command, here-document, or `&&`/`|`
continuation says so).

**Fix.** A new `parser::IncrementalParser` drives the existing grammar one unit
at a time:

* `Parser::parse_item` was split out of `Parser::parse_program` (which is now a
  three-line loop over it), so both the batch and incremental paths share exactly
  one implementation of the item grammar.
* `IncrementalParser` keeps the source's tokens **unexpanded** (`orig`) plus a
  `work` stream that is `orig[pos..]` alias-expanded under a remembered alias
  state. `next_unit(aliases)` re-expands only when that state changes, so a
  script with a stable (or empty) alias table pays for one expansion pass total.
* `lexer::expand_aliases_tracked` is a new origin-tracking variant of
  `expand_aliases`: it returns, per output token, the input index it came from
  (`None` for tokens an alias spliced in). That is what lets the parser resume in
  original coordinates after each unit.
* `run_source_flow_out` now loops `next_unit` → `exec_program_top`, re-reading
  `aliases_enabled()`/`self.aliases` every unit.

**Trap for a future session (cost me a debugging cycle).** The first cut gave
*every* alias-spliced token origin `None`. That is wrong for the **first** one:
when the alias state later changed and the stream was rebuilt, "resume at the
next `Some` origin" skipped past the whole replacement and **dropped the alias
word itself** — `shopt -u expand_aliases` followed by `hi` re-ran the expansion
instead of failing. The first replacement token stands in for the alias word and
must inherit its origin; only the tokens after it are origin-less. (An *empty*
alias value contributes no token and correctly needs no mark.)

**Second trap.** A unit that ends without a separator is only legal at EOF. The
first cut let `echo one; echo two )` execute `echo one` before noticing the stray
`)`, because `)` is an accepted item terminator. `IncrementalParser::next_unit`
now rejects a non-EOF no-separator ending *before* handing the unit back, so a
partially-parsed line runs nothing — as in bash.

**Also fixed here (same corpus case).** `command -v`/`-V` had no keyword or alias
branch, so `command -v while` exited 1 instead of 0, and `type`/`type -t` never
reported aliases at all. Both now follow bash's `describe_command()` precedence,
**alias > keyword > function > builtin > `$PATH` file**, via a shared
`Shell::describe_alias` (aliases are only reported while `expand_aliases` is in
effect — with it off, bash says "not found"). The duplicated keyword list in
`builtin_type`/`builtin_compgen` was hoisted to one `SHELL_KEYWORDS` const.

**Test-harness fidelity fix.** The unit-test `run()` helper called `parse()` plus
`exec_program_top` directly, bypassing the real driver — which is why none of
this was caught by 730 passing tests. It now goes through `Shell::run_source_out`
like a real script (a syntax error is status 2 + a diagnostic, not a panic), and
a sibling `run_script()` additionally clears `repl_interactive` for tests that
need a script's `expand_aliases`-off default. All 730 pre-existing tests passed
unchanged through the real driver.

**Verification.** `shopt_expand_aliases_takes_effect_for_later_lines`,
`commands_before_a_syntax_error_still_run`,
`type_and_command_v_report_keywords_and_aliases` (interp.rs); corpus cases
`command-type-alias.sh` (alias expansion incl. the trailing-blank and same-line
rules, `shopt -u`, `type`/`type -a`/`command -v`/`-V`) and the new
`parse-order.sh` (function-body vs call-time resolution, `eval` timing,
here-doc/compound units, and the syntax-error ordering — byte-identical stderr,
including bash's echoed offending source line). 733 + 15 tests green, clippy
clean, 43/43 corpus cases match.
