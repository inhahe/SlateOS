### TD-OILS-AN-ARITHMETIC-ERROR-UNDER-@P-STILL-ABANDONS-THE-COMMAND. `v='A${a[0p]}B'; echo "[${v@P}]"` prints nothing where bash prints `[A0B]` — 2026-08-09 — ✅ FIXED 2026-08-09

**Where:** `userspace/oils/src/interp.rs`. `Shell::prompt_expanding` /
`Shell::prompt_failed` stop the *jump* for a failed expansion, but an
**arithmetic** error raised while evaluating a subscript (or any arithmetic
inside the re-read prompt string) is not routed through them, so it aborts the
enclosing command.

**Reproduce** (`a=(0 1 2)`):

```text
v='A${a[0p]}B';           echo "[${v@P}]"
w='A${a[${a[9x]}p]}B';    echo "[${w@P}]"
i='A${a[\`fi\`]}B';       echo "[${i@P}]"      # value has a literal backslash-backquote
```

Both shells print the same `0p: value too great for base (error token is "0p")`
diagnostics — bash then prints `[A0B]` (the failed subscript evaluating to 0,
the word around it intact), osh prints nothing at all: the `echo` never runs.
The third row is the same defect reached through the arithmetic tokenizer's
`` `fi` ``: `` syntax error: operand expected ``, then bash `[A0B]` and osh
nothing.

**The recorded hypothesis was wrong, and the measurement replaced it.** The
entry above guessed at "`SEVAL_NOLONGJMP`-equivalent protection for the whole
string". What bash actually does is two unrelated things, and the difference
between them is exactly the difference between `[A0B]` and the literal text:

* **The boundary.** `expand_prompt_string` is the only entry into
  `expand_word_internal` that *keeps* the two error returns instead of jumping
  on them — `if (value == &expand_word_error || value == &expand_word_fatal)
  { value = make_word_list (make_bare_word (string), …); return value; }`
  (subst.c:4463-4467). Every other caller reaches `call_expand_word_internal`,
  which turns the same two into `exp_jump_to_top_level (DISCARD)` / `(FORCE_EOF)`
  and never consults `no_longjmp_on_fatal_error` (subst.c:4326-4334). So the
  interception is a property of the *boundary*, not of the flag — which is why
  an expansion error inside a `$( … )` that a prompt **runs** still ends that
  substitution (measured: `n='A$(echo "x${q@}y"; echo tail)B'` under `@P` gives
  `AB`, no `tail`). Every class that ends `expand_word_internal` early therefore
  leaves the prompt as the *undecoded text it was handed*, healthy neighbours
  lost with it: `X${q}Y${q!}Z${q}W` is `X${q}Y${q!}Z${q}W`, not `XQQYZQQW`.
* **The subscript.** `array_expand_index` is the one *arithmetic* caller that
  reads the flag for itself — `if (no_longjmp_on_fatal_error) return 0;`
  (arrayfunc.c:1372) — so the subscript is simply **0**, the word around it is
  expanded in full, and there is no error return to intercept at all. That, and
  not the boundary, is what makes `A${a[0p]}B` into `A0B`. A `${p:0p:1}` bound
  has no such guard and does fall back to the literal text.

Two things do **not** revert, both measured:

* `set -e`. `report_error` exits *at the diagnostic*
  (`if (exit_immediately_on_error) … exit_shell (…)`, error.c:201), so there is
  no return value left to keep. An arithmetic diagnostic is `internal_error`
  (`evalerror`, expr.c:1130-1141), which has no such clause — so `set -e;
  v='A$((1+))B'; echo "${v@P}"` still prints `A$((1+))B` and keeps running.
* `set -o posix`. That promotion is spelled as the *return value*
  (`(posixly_correct && interactive_shell == 0) ? &expand_wdesc_fatal :
  &expand_wdesc_error`, subst.c:10051), so the prompt intercepts it like any
  other and the shell survives.

**Fixed** in `userspace/oils/src/interp.rs`, in three pieces:

* `Shell::prompt_expand` scopes `unbound_error`/`discard_error` around its
  `expand_double_quoted` call and, if either was raised inside, keeps the
  decoded text instead of the expansion — the boundary above.
* A new `Shell::shell_error_exit`, set by `Shell::note_shell_error`'s errexit
  branch and by the two sites that arm a `report_error`-class abort directly
  (`Shell::arm_unbound_abort`, `Shell::bad_substitution_with`'s fatal branch,
  both through the new `Shell::note_report_error_exit`), is the one abort the
  boundary lets through.
* `Shell::eval_arith_index_text_checked` answers `Some(0)` and disarms the
  discard under `prompt_expanding` — `arrayfunc.c:1372`, above. Deliberately
  *not* in `eval_arith_expr_checked`, which a substring bound also uses.

**The interception was too wide, and a second measurement narrowed it.** The
three pieces above intercept *every* failure met anywhere under `${x@P}`. Bash
intercepts only the failures raised on the **prompt's own walk**. The reason is
the same boundary read the other way: `expand_prompt_string` is the only
*non-nested* entry into `expand_word_internal`. A pattern, an operand, a
replacement, a `${x:?word}` message, an associative subscript and an arithmetic
string are each handed to a **separate** call, and that call is entered through
`call_expand_word_internal`, whose answer to the two error returns is
`exp_jump_to_top_level` (subst.c:4326-4334) — which ends in `jump_to_top_level
(v)` unconditionally (subst.c:12141-12157) and never reads the flag. So a
failure raised inside one of those **jumps**, under `${x@P}` exactly as
anywhere else. Measured, with `y=Y`:

```text
v='A$((1+))B';       printf '[%s] rc=%s\n' "${v@P}" "$?"   # [A$((1+))B] rc=0
v='A${y#$((1+))}B';  printf '[%s] rc=%s\n' "${v@P}" "$?"   # (nothing; the printf never runs)
```

`$(( … ))` is that rule read twice over: `expand_arith_string` only makes a
nested call when the string holds an `ARITH_EXP_CHAR` — `$`, a backquote,
`CTLESC`, `~` (subst.c:3824, 4033-4051) — and the `evalexp` afterwards is the
*caller's* own frame either way. Hence an arithmetic error is interceptable and
a bad substitution in the same expression is not.

Three more differences fell out of measuring that boundary, each its own fix:

* **An arithmetic substitution evaluates with no name tag.**
  `savecmd = this_command_name; this_command_name = (char *)NULL;` around
  `evalexp` (subst.c:10610-10617), where `parameter_brace_substring` sets
  `this_command_name = varname` across `verify_substring_values`
  (subst.c:8802-8803). So `${s:1+:1}` says `s: 1+: syntax error` but
  `${s:$((1+)):1}` says plain `1+: syntax error`. `Shell::arith_sub` now takes
  `arith_cmd` for the evaluation and restores it after.
* **Nothing after the jump in the enclosing construct happens.** Whatever the
  construct was going to do with the string it never got is downstream of it:
  `${u:=…}` does not store (measured — `u` stays unset), `${u:?…}` does not
  print its message, an associative `${m[…]}` does not judge the empty key.
  Guards on `Shell::expansion_failed()` after the operand/message/key
  expansion in `ParamOp::AssignDefault`, `ParamOp::ErrorIfUnset`,
  `Shell::param_elem_lookup` and the `${name[index]}` associative read.
* **The nested call names its own word.** The `WORD_DESC` it is handed is the
  replacement alone, so `A${y//Y/${q!}}B` reports `` `${q!}' `` and not the
  whole word. `Shell::expand_replacement` now brackets its body in
  `begin_word`/`end_word` like the other nested readers.

**A jumped prompt leaves `no_longjmp_on_fatal_error` raised for good.**
`expand_prompt_string` lowers the flag on the line *after* the expansion
returns (subst.c:4459-4461), so a failure that jumped past that line never
lowers it. It then shows wherever the flag is read *directly* — chiefly
`array_expand_index` (arrayfunc.c:1372), which from then on answers a bad
subscript with 0 instead of jumping. Measured: `a=(9 8 7); ( echo "${a[1+]}" )`
is silent with status 1, but after one `v='A${y#$((1+))}B'; printf '[%s]\n'
"${v@P}"` the same subshell reports the error and then prints `9` with status
0. `Shell::prompt_expand` models it by not restoring `prompt_expanding` when
the walk jumped. The two error *returns* are unaffected —
`call_expand_word_internal` turns them into jumps without consulting the flag.

The nesting is carried by a new `Shell::expansion_jumped` flag and a
`Shell::expand_call` wrapper, which every nested reader
(`expand_word_pattern`, `expand_operand_fields`, `expand_replacement`,
`expand_to_string`, `expand_to_arith_string`, `expand_arith_params`) is now
routed through; `prompt_expand` restores the saved errors only when the walk
did **not** jump.

**Corpus:** `a-prompt-expansion-keeps-its-text-when-the-expansion-fails.sh`,
`a-nested-word-expansion-is-its-own-call-so-its-failure-jumps.sh`.

**Found by** the probe series for
TD-OILS-A-BRACE-SUBSCRIPT-IS-NOT-EXPANDED-AS-A-STRING-OF-ITS-OWN.
