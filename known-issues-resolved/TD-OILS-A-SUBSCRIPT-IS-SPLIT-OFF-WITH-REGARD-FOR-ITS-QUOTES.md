### TD-OILS-A-SUBSCRIPT-IS-SPLIT-OFF-WITH-REGARD-FOR-ITS-QUOTES. `printf -v 'n["1]' X` wrote index 0 where bash says `not a valid identifier` — 2026-08-05 — ✅ **FIXED 2026-08-05**

**Where:** `userspace/oils/src/interp.rs` — the new `skip_subscript` (beside
`split_assignment_target`, near the bottom of the file) and the five splits that
now use it: `split_assignment_target`, `attr_assignment_split`,
`subscript_base`, the `declare`/`local` operand split and `unset`'s element
split.

**What.** bash matches a name-with-subscript with `skipsubscript`, which steps
*over* quoted runs, backslashes and `` `…` ``/`$(…)`/`${…}` rather than through
them. So a `]` inside quotes does not close the subscript, and a quote that
never ends means the subscript never closes at all — which makes the whole
operand not a name, refused as one *before* anything is expanded. Measured with
`n=(a b c)`:

```text
                          bash                              osh (before)
printf -v 'n["1]' X       `printf: …: not a valid           wrote n[0],
                          identifier`, status 2             status 0
printf -v 'n[$(echo 1]' Y same                              wrote n[0]
declare 'n["1]=U'         `declare: …: not a valid          wrote n[0]
                          identifier`
export 'n["1]=E'          blames the *whole* operand        blamed `n["1]`
declare -n r='n["1]'      `declare: …: invalid variable     accepted; read
                          name for name reference`          element 0
unset -v 'n["1]'          `unset: …: not a valid            silent, status 0
                          identifier`
q='n["1]'; echo "${!q}"   `n["1]: invalid variable name`    read element 1
```

Five splits each scanned for `[` and required the last byte to be `]`, so
`n["1]` looked like the name `n` with the subscript `"1`.

**Fixed 2026-08-05.** `skip_subscript` is bash's `skipsubscript` —
`skip_matched_pair` on `[`/`]`. It is written as an explicit stack of what each
open construct is waiting for rather than the mutual recursion bash uses, so a
pathologically nested operand costs memory instead of stack. Nothing inside a
`$(…)` or `${…}` can close the subscript, so `[`/`]` count only while one is
the innermost thing open — bash's arrangement too, `extract_command_subst`
swallowing its body whole before the bracket counter ever sees it.

The five callers ask it where the subscript closes and require that to be the
last byte. What *is* well-quoted keeps working and is now reachable from every
one of them: `m["]"]` is the key `]`, `m["k k"]` the key `k k`.

`unset` keeps its own rule about *when* to check — only an explicit `-v` makes
it judge the spelling — so `unset 'n["1]'` is still the quiet no-op bash makes
of it while `unset -v 'n["1]'` complains.

**Corpus:** `a-subscript-is-split-off-with-regard-for-its-quotes.sh`.
