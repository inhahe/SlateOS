### TD-OILS-DECL-DIAGNOSTIC-ESCAPES-REDIRECTION. `declare`'s invalid-option and refusal messages ignore the command's own redirections — 2026-07-31 — ✅ RESOLVED 2026-07-31

**Where:** `userspace/oils/src/interp.rs` — `exec_declare_with_arrays_scoped`.
A `declare` carrying a compound `name=(…)` operand is dispatched straight
to that function from `exec_simple`, so it never reached
`run_builtin_body`, which is where a builtin's scoped stderr push lives.
Every diagnostic the command raised therefore went to the *enclosing*
stderr.

```sh
declare -i+i n=(2+3) 2>/dev/null    # bash: silent.  osh: printed the
                                   # "-+: invalid option" + usage pair
declare +a -l +l k=(AB) 2>/dev/null # bash: silent.  osh: printed
                                   # "cannot destroy array variables in this way"
```

**Measured rule (bash 5.2.37, `target/dvscratch/px78.sh`–`px82.sh`).**
A `declare` with a compound operand speaks with two voices, and only one
of them is redirectable — because bash binds `name=(…)` while it is still
*expanding the words*, and installs the command's redirections only
afterwards, just before running the builtin:

* **The compound-assignment machinery's diagnostics escape** `2>/dev/null`.
  Confirmed: the untagged conversion refusal (`ci: cannot convert indexed
  to associative array`), the function-tagged `noassign` and
  readonly-shadow lines, the circular-nameref warning, the
  element-reference refusal (`` `ng[1]': not a valid identifier ``), the
  arithmetic error from an `-i` literal, and the readonly-target refusal.
* **The builtin's own diagnostics are redirected**: invalid options, the
  `+a`/`+A` destroy refusal, the `-a`/`-A` self-conflict, the phase-3
  refusals, and `declare -p`'s "not found".
* **The two refusals bash reports *twice*** (`noassign` and the
  readonly-shadow one) are silenced by halves: the machinery's tagged line
  survives `2>/dev/null`, the builtin's `local: …` line does not.
* Those builtin halves also come **after every machinery half**, not
  interleaved operand by operand, because bash's builtin runs only once
  the whole word list has expanded: `local ra=(1) rb=(2)` with both
  readonly prints both `f: …` lines and only then both `local: …` lines.
* The trace line of `set -x` is written **before** the redirect is in
  place, so it is not redirected either.

**Fix.** `run_builtin_body`'s inline stderr push was extracted into
`Shell::push_builtin_stderr`, and `exec_declare_with_arrays_scoped` now
installs it around phases 2 and 3 only — after the compound literals have
bound and after the xtrace line, popping at both exits. The builtin's half
of the two double-reported phase-1 refusals is collected into
`builtin_refusals` during phase 1 and emitted under that push, which fixes
the ordering at the same time. Pinned by
`userspace/oils/tests/corpus/declare-redirected-diagnostics.sh`.

Note this means the corpus's "group the diagnostics through `e()` rather
than redirecting per command" idiom is still needed — but only for the
*phase 1* shapes, which genuinely escape a per-command redirect in bash
too.

**Still open, separately:** `local` used outside a function *with* a
compound operand — see TD-OILS-DECL-LOCAL-OUTSIDE-FUNCTION-SKIPS-BINDING.
