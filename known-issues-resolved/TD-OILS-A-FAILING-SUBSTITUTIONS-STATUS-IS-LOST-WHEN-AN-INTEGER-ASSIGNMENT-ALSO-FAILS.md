### TD-OILS-A-FAILING-SUBSTITUTIONS-STATUS-IS-LOST-WHEN-AN-INTEGER-ASSIGNMENT-ALSO-FAILS. `` declare -i n; n=1+`fi` `` — 2026-08-07 — ✅ FIXED 2026-08-07

**Where:** `userspace/oils/src/interp.rs` — the status an assignment returns when
the variable is integer-attributed and the arithmetic evaluation of its value
fails.

**What.** An assignment normally returns the status of the last substitution its
value performed. When the variable carries `-i`, the assigned text is then
evaluated as arithmetic, and that evaluation can fail in its own right. bash
keeps the substitution's status through that second failure; osh replaces it
with the arithmetic one:

```text
declare -i n; n=1+`fi`
  bash: command substitution: line 1: syntax error near unexpected token `fi'
        line 1: 1+: syntax error: operand expected (error token is "+")   rc=2
  osh : (same two diagnostics)                                            rc=1
```

Both diagnostics match byte for byte — this is purely the status.

**Only this shape.** Measured against bash 5.2.37, every neighbour already
agrees at rc=2: `` x=`fi` `` (no integer attribute), `` n=`fi` `` and
`` n=`fi`+1 `` (integer, but the arithmetic of an empty/`+1` value does not
itself fail the same way), and `` declare n; n=1+`fi` `` (no integer attribute,
so `1+` is stored verbatim and never evaluated). The divergence needs *both*
failures — the substitution's and the arithmetic's — in one assignment.

**Why bash does this.** The failure is not *reported* as a status at all — it is
an abort. `make_variable_value` (variables.c:2941) calls `evalexp` on the value
when the slot is `integer_p`, and on `expok == 0` does `top_level_cleanup ();
jump_to_top_level (DISCARD)` (variables.c:2959–2968) unless the caller passed
`ASS_NOLONGJMP` — which is how `(( … ))` and `let` get a mere status where an
assignment gets an unwind. `top_level_cleanup` pops the saved `top_level`
jump buffer that `parse_and_execute` installed, which is why the unwind passes
straight through an `eval` or a `source` and always lands in the reader loop.
And the reader loop's handler does not force a status — it only supplies one
when there is none (eval.c:103):

```c
	    case DISCARD:
	      /* Make sure the exit status is reset to a non-zero value, but
		 leave existing non-zero values (e.g., > 128 on signal)
		 alone. */
	      if (last_command_exit_value == 0)
		set_exit_status (EXECUTION_FAILURE);
```

So the 2 the failed substitution left in `last_command_exit_value` survives, and
the 1 that `declare -i b=2+` produces is merely the `== 0` fallback.

**Fixed by** `Shell::arm_int_bind_discard` (`interp.rs`) carrying `1` only when
`self.last_status` is 0, and the current status otherwise — the direct
transcription of the C above. bash decides this at the reader loop rather than
at the raise, but nothing runs in between, so arming with the resolved status is
equivalent; `arm_discard`'s explicit-status contract is untouched.

**The rule is wider than this entry first recorded.** Written up as "bash keeps
the substitution's status", it looked like a fixed 2. Measuring the shape space
before touching anything showed it is *any* status the value's expansion left:

```text
declare -i n; n=2+                  rc=1   nothing ran; the fallback
declare -i n; n=1+`false`           rc=1   the substitution's own 1
declare -i n; n=1+`fi`              rc=2   a parse failure's 2
declare -i n; n=1+`exit 3`          rc=3   and any other status too
```

The same measurement disposed of a plausible wrong reading of eval.c:103 —
that an *earlier* non-zero status would survive too. `(exit 7); declare -i n;
n=2+` is 1, not 7, because the `declare` between them succeeds and resets the
status; reading `last_status` at the raise reads it after the value was
expanded, which is where bash reads it, so this falls out rather than needing
special handling.

Covered by `tests/corpus/an-integer-assignment-keeps-the-status-its-value-left.sh`,
which also pins the append, `declare`, array-element and `let` spellings, and
the unwind still passing through `eval` and a function.
