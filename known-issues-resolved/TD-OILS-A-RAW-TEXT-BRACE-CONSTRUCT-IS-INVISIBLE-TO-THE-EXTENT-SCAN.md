### TD-OILS-A-RAW-TEXT-BRACE-CONSTRUCT-IS-INVISIBLE-TO-THE-EXTENT-SCAN. `${y@$(fi)}` reports one diagnostic short — 2026-08-09 — ✅ FIXED 2026-08-09

**Where:** `userspace/oils/src/ast.rs`. `WordPart::ArithSub { expr }` keeps
**unparsed source**, so a `$( … )` inside one is not a `WordPart::CommandSub` and
`Shell::brace_scanned_subs` cannot find it. `WordPart::BadTransform` and
`BulkOp::BadTransform` were the same defect and are fixed — see below.

**Reproduce** (`y=Y`, `q=QQ`, `n=(1 2)`, expanded with `@P`) — the two
`@`-transform rows that opened this entry (`A${q@$(fi)}B` and
`A${n[@]@$(fi)}B`) now match; what is left is the arithmetic body:

```text
A${x@$((1+$(fi)))}B                        … a `$((` inside a bad transform's
  bash: command substitution: line 3: syntax error near unexpected token `fi'
        command substitution: line 3: `fi)))}B'
        line 2: A${x@$((1+$(fi)))}B: bad substitution
        A${x@$((1+$(fi)))}B
  osh:  line 2: A${x@$((1+$(fi)))}B: bad substitution
        A${x@$((1+$(fi)))}B                  operand — the operand is a parsed
                                             Word now, but its ArithSub inside
                                             it is still raw text

A${y:-$((1+$(fi)))}B
  bash: command substitution: line 3: syntax error near unexpected token `fi'
        command substitution: line 3: `fi)))}B'
        line 2: A${y:-$((1+$(fi)))}B: bad substitution
        [A${y:-$((1+$(fi)))}B]
  osh:  [AYB]                              (no diagnostics at all)

A$((1+$(fi)))B                             … the same construct at string level
  bash: command substitution: line 2: syntax error near unexpected token `fi'
        command substitution: line 2: `fi)))B'
        [A]
        rc=0
  osh:  command substitution: line 2: syntax error near unexpected token `fi'
        command substitution: line 2: `fi)'
        (nothing — neither the `[…]` nor the `rc=` after it runs)
```

Note the same construct written *directly in a script* — `echo "A$((1+$(fi)))B"`
— is a parse-time error in **both** shells, blaming the whole line, and they
agree there. Only the expansion-time reading (`@P`, and by extension anything
that re-reads text) differs; `eval` agrees too, because it re-parses.

The last row is the same defect seen from the other side: osh *does* find that
`$( … )`, but only by parsing the arithmetic expression on its own, so the tail
it blames is the expression's remainder (`` `fi)' ``) rather than the enclosing
string's (`` `fi)))B' ``). bash reaches it during the scan, where
`extract_delimited_string` carries `SX_COMMAND` and recurses into a `$( … )` —
which is also the correction to the comment in `Shell::brace_extent_scan` that
says a `$((`'s extent "cannot fail". The consumption differs too, and in the
opposite direction from the brace case: bash consumed the rest of the string and
still finished the command (`[A]`, `rc=0`), where osh's failure ends it.

Measured fresh 2026-08-09 (the earlier note here had osh printing the text back;
it does not any more — the nested-call jump model of
TD-OILS-AN-ARITHMETIC-ERROR-UNDER-@P-STILL-ABANDONS-THE-COMMAND now makes this
one jump. Whether the jump or the text is right is exactly what part 2b decides:
bash's is a *scan* failure, which does not jump, not an expansion failure.)

**`WordPart::BadSubst(Str)` is *not* part of this** — measured, and it is bash's
own structure rather than an accident. The operand scan is
`extract_dollar_brace_string` at subst.c:10063, and every spelling osh classifies
as `BadSubst` fails *before* it: `${q!$(fi)}` and `${!q*$(fi)}` are
`valid_brace_expansion_word (…) == 0 → goto bad_substitution` (subst.c:9816-9820)
because `!` does not end the name, and `${#a[i]extra}` is
`valid_length_expression (…) == 0` (subst.c:9704). So bash reports only
`bad substitution` for those and never reads the `$(` — which is what osh already
does. A transform is different precisely because `@` *does* end the name
(subst.c:10056), so the operand scan runs.

**Fixed already (2026-08-09), and separately:** the other half of the original
first row — osh *exiting* where bash carried on — was not about the scan at all.
See TD-OILS-AN-ARITHMETIC-ERROR-UNDER-@P-STILL-ABANDONS-THE-COMMAND, which
records the boundary rule (`expand_prompt_string` keeps the two error returns)
that covers the fatal `@`-transform along with every other class.

**Fixed (the `@`-transform half), 2026-08-09.** `WordPart::BadTransform` and
`BulkOp::BadTransform` no longer carry the whole `${ … }` as raw text; the
operand after the `@` is now a parsed `op: Box<Word>` (`ast.rs`), built by the
new `parser.rs` helper `bad_transform_operand`, which reads the text after the
`@` with `word_verbatim_from_source_at` under the *enclosing* text's quoting
(there is no `getpattern` here — nothing ever expands the operand). That is
enough structure for the scan: `first_scanned_arith` now descends into the
operand, and `Shell::brace_scanned_subs` finds a `$( … )` in it like any other.

The rule this implements, and the reason it is the *operand* rather than the
operator that decides: bash's `extract_dollar_brace_string` walks the whole body
looking for the `}` and reads a `$( … )` on the way with a **real parse**
(`extract_command_subst`, subst.c:1896-1902) *before* `parameter_brace_transform`
ever looks at the operator. So both things happen, in that order — the failed
extent is reported, and only then the `bad substitution`. And a failed extent
read consumes to the end of the string, so the brace never closes and

```c
value = extract_dollar_brace_string (string, &sindex, quoted, …);
if (string[sindex] == RBRACE) sindex++;
else goto bad_substitution;                     /* subst.c:9913-9918 */
```

fires — which is why the diagnostic names the *whole* word, and why even an
**unset** parameter complains (`${nope@$(fi)}` reports; `${nope@Z}` is quietly
empty), the verdict having been reached before the parameter was looked up. Two
spellings are only stepped over rather than read, so neither reports: a
backquote (`string_extract`, subst.c:1886) and a single-quoted run
(`skip_single_quoted`, subst.c:1926-1938).

Dropping the raw text also removed the `deferred_body_mut` special case for this
construct (so `parse_arith_comsubs` no longer double-parses it, and cannot
double-gather here-documents through it), and the re-print splice came for free
through the parsed operand — `declare -f` prints `$( ( echo 2 ))` where the
source said `$( (echo 2) )`, byte-identical to bash, because `part_src` now
rebuilds `${name[sub]@op}` from its parts like every other operator's.
`Shell::bulk_elements` gained a `star: bool` parameter so the bulk arm can name
`x[@]` vs `x[*]` in the diagnostic it rebuilds. Corpus:
`a-bad-transform-operand-is-read-before-the-operator-is-judged.sh`.

**Fixed (the arithmetic half), 2026-08-09.** `WordPart::ArithSub` now carries
`parts: Vec<WordPart>` beside `expr` — the same bytes, cut into literal runs and
one `CmdSubBody::Unread` per `$( … )` the *expansion-time* scan will read there
(`parser.rs`'s new `arith_unread_subs`, whose invariant is `parts_src(parts) ==
expr`). No second raw-text scanner was needed: `Shell::brace_scanned_subs` walks
the parts like any others, and `unparse::attach_comsub_tails` fills each one's
tail for free, because it measures a remainder by rendering the whole word with a
sentinel in place — which is exactly why the body had to become parts rather than
annotated raw text.

Making the parts reachable meant recording the spans in the first place: the
lexer's `CmdSubSpan` gained a `kind: SubBody`, and the two `if !self.here_text`
guards that had been suppressing the record were dropped, since a scan that is
already running never changes `here_text`. `parse_arith_comsubs` now takes only
`SubBody::Eager` spans, so an eagerly-parsed body still contributes its re-print
to `expr` and no part (its second read is the arithmetic expansion's own), while
an unread one contributes a part and no splice — the two never overlap.

The two spellings answer the scan differently, and that is bash's structure:

* `$(( … ))` is reached through `extract_command_subst`, which for a body
  starting `(` is `extract_delimited_string (string, sindex, "$(", "(", ")",
  xflags|SX_COMMAND)` (subst.c:1284-1286). The paren count cannot fail, but
  `SX_COMMAND` recurses into a nested `$( … )` with a real parse over the *whole
  enclosing string* (subst.c:1431-1437) — hence `` `fi)))B' `` rather than
  `` `fi)' ``.
* `$[ … ]` is `extract_delimited_string (string, sindex, "$[", "[", "]", 0)`
  (`extract_arithmetic_subst`, subst.c:1299-1304) — flags `0`, **no**
  `SX_COMMAND` — so it recurses into nothing and its `$( … )` is read only when
  the expression is expanded. `Shell::arith_extent_scan` takes a `bracket: bool`
  and returns immediately for it. The parts are still recorded, because an
  *enclosing* `${ … }` scan does read them: `A${z:-$[1+$(fi)]}B` reports the
  brace's `` `fi)]}B' `` where the bare `A$[1+$(fi)]B` reports `` `fi)' ``.

The ending differs from the brace's too, and needed its own path.
`Shell::brace_extent_scan` was split: `Shell::extent_read_of` does the reading,
and the two callers supply the ending. A `${ … }` that cannot close is a `bad
substitution` plus `prompt_failed`; an arithmetic that ran off the end is *simply
nothing* — nothing was asked to close but the parens, and they closed — so
`A$((1+$(fi)))B` under `@P` prints one command-substitution diagnostic and then
`[A]`, where the same failure under a `${ … }` prints the whole undecoded word.
`brace_extent_scan` therefore early-returns for `WordPart::ArithSub` exactly as
it already did for `WordPart::DoubleQuoted`, so an arithmetic met at an expansion
door is scanned only by its own arm — while one nested *inside* a `${ … }` still
gets the brace's ending, `brace_scanned_subs` having descended into its parts.

Corpus: `an-arithmetic-extent-read-parses-a-command-substitution-inside-it.sh`.
Two divergences uncovered on the way are **not** this entry and are filed
separately: TD-OILS-A-BRACKET-ARITHMETIC-GIVES-UP-AFTER-A-FAILED-SUBSTITUTION and
TD-OILS-A-FAILED-EXTENT-READ-RUNS-TO-THE-END-OF-THE-STRING.
