### TD-OILS-A-BRACKET-ARITHMETIC-GIVES-UP-AFTER-A-FAILED-SUBSTITUTION. `v='A$[1+$(fi)]B'; echo "${v@P}"` stops at the first diagnostic where bash reports twice and evaluates on — 2026-08-09 — ✅ FIXED 2026-08-09

**Where:** `userspace/oils/src/interp.rs`, `Shell::arith_dolparen` — the tail of
it, which answers a body that will not parse with an unconditional
`self.arm_discard(1); return None;` and so has no `prompt_expanding` branch at
all. Reached from `expand_arith_params_inner`'s `'('` arm, whose
`let Some(sub) = … else { break }` is the give-up. Visible only through `$[ … ]`
now that `$(( … ))` is read by the extent scan first (see
TD-OILS-A-RAW-TEXT-BRACE-CONSTRUCT-IS-INVISIBLE-TO-THE-EXTENT-SCAN), but the
defect is in the shared path.

**Reproduce** (expanded with `@P`, so `no_longjmp_on_fatal_error` is on):

```text
v='A$[1+$(fi)]B'; printf '[%s]\n' "${v@P}"; echo "rc=$?"
  bash: command substitution: line 2: syntax error near unexpected token `fi'
        command substitution: line 2: `fi)'
        command substitution: line 1: syntax error near unexpected token `fi'
        command substitution: line 1: `fi'
        line 1: 1+: syntax error: operand expected (error token is "+")
        [A$[1+$(fi)]B]
        rc=0
  osh:  command substitution: line 2: syntax error near unexpected token `fi'
        command substitution: line 2: `fi)'
        (nothing after — neither the `[…]` nor the `rc=`)

v='A$[1+$(fi)+3]B'   bash quotes `fi)+3' then `fi)+'   (osh: only the first)
v='A$[$(fi)]B'       bash quotes `fi)'  then `fi', then prints [A0B]
```

**Why.** `extract_arithmetic_subst` is
`extract_delimited_string (string, sindex, "$[", "[", "]", 0)`
(subst.c:1299-1304) — flags `0`, no `SX_COMMAND` — so unlike `$((` it recurses
into nothing and the nested `$( … )` is read only when the expression itself is
expanded, `expand_arith_string (temp, Q_DOUBLE_QUOTES|Q_ARITH)` (subst.c:10662).
That read is an ordinary *string-level* one, and the two quoted texts are the
string-level pair this repo already models: the extent read echoes the whole
remainder and the child that runs it echoes one byte less (see
TD-OILS-A-FAILED-EXTENT-PARSE-CONSUMES-THE-REST-OF-THE-STRING and
`Shell::failed_extent_split`). bash then carries on with the empty result and
evaluates what is left — `1+` — giving the arithmetic error and, the expansion
having failed, the whole undecoded word. osh instead treats the first failure as
fatal to the word.

**Fixed, 2026-08-09.** `Shell::arith_dolparen`'s failure tail was given the
`prompt_expanding` branch `Shell::comsub_reparse_read` already had — the same
guard, from the same two lines:

```c
if ((flags & SX_NOLONGJMP) == 0)
  jump_to_top_level (-nc);              /* parse.y:4330 */
```

Under a prompt expansion it now reports (as it already did), sets
`Shell::extent_consumed`, and runs `Shell::failed_extent_split` over **`rest`** —
everything past the `$(`, which is what `parse_string` was handed, not the extent
`scan_cmdsub_body` carved out — returning that child's output rather than `None`.
That accounts for both quoted texts exactly: the parse names `rest`'s remainder
(`` `fi)+3' ``) and the child runs it less a byte (`` `fi)+' ``).
`expand_arith_params_inner` gained the `extent_consumed` break/restore pair
`Shell::expand_word_parts` already used, so the rest of the *expression* is
consumed rather than expanded, and `Shell::arith_sub` evaluates what was
accumulated before the `$(` — `1+`, hence bash's operand-expected error, or
nothing at all, hence `[A0B]` for `A$[$(fi)]B`.

Two things the measurement settled that the fix had to respect:

* **errexit still wins**, and is therefore asked first. It does not go through
  the suppressed jump at all — `parser_error` ends with a direct `exit_shell` —
  so `set -e` on the same line prints the first diagnostic line and nothing else.
* **The consumption stops at the expression.** `expand_arith_string` is one
  `expand_word_internal` call over the expression alone, so it owns its own
  `sindex`: `A$[$(fi)]B${y}C` is `[A0BYC]`, the `${y}` after the arithmetic
  expanding normally. Hence the save/restore rather than a bare set.

Corpus:
`a-bracket-arithmetic-reads-its-substitutions-when-the-expression-is-expanded.sh`
(12 probes, including the pair's two texts, the truncated evaluation, a
substitution before the bad one having already run, the brace's ending when one
is wrapped in `${z:- … }`, and a here-document body — unread text, but not a
prompt — where the jump still stands).
