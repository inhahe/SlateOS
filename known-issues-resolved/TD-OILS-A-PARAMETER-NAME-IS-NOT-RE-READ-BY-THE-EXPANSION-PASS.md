### TD-OILS-A-PARAMETER-NAME-IS-NOT-RE-READ-BY-THE-EXPANSION-PASS. `echo "${a[}"x"]}"` expands the wrong name — 2026-08-08 — ✅ FIXED 2026-08-08

**Where:** was `userspace/oils/src/interp.rs`, the `${ … }` expansion, which
used the name the *parser* carved out; is now `Shell::reread_word`
(interp.rs), `ParseOpts::reread` and `read_dollar_brace_body`'s subscript jump
(lexer.rs). bash's `parameter_brace_expand`
(subst.c:9539) carves it out again with `string_extract` under `SX_VARNAME`
(subst.c:795), and that scan skips a closed subscript wherever the `]` is —
including past a closing double quote.

**What.** The sibling of
TD-OILS-AN-UNCLOSED-SUBSCRIPT-IN-A-QUOTED-BRACE-BODY-IS-NOT-A-RUNAWAY-SCAN,
found while fixing it, and the one shape that fix does not reach. Where that
one is the extent pass and the parser disagreeing about where the `${ … }`
*ends*, this is them disagreeing about what its *name* is:

```text
                                bash                          osh
echo "${a[}"x"]}"               }x: syntax error …            ${a[}: bad substitution
echo ${a[}x]}                   }x: syntax error …            ${a[}x]}: bad substitution
h[}x]=HIT; echo "${h[}"x"]}"    HIT                           ${h[}: bad substitution
h[}x]=HIT; echo "${h[}"x"]:-D}" HIT                           ${h[}: bad substitution
h[}x]=HIT; echo "${!h[}"x"]}"   (empty, rc=0)                 ${h[}: bad substitution
echo "${#h[}"x"]}"              ${#h[}: bad substitution      ${#h[}: bad substitution
```

The parser reads the name as `a[` and stops at the first `}`, which is not a
valid brace-expansion word — hence osh's bad substitution. bash's re-read hits
the `[`, calls `skipsubscript` over the whole word, finds the `]` three
characters past the closing quote, and comes back with the name `a[}"x"]` — a
well-formed array reference whose *subscript* is `}"x"`. On an indexed array
that is evaluated as arithmetic and the arithmetic evaluator complains; on an
**associative** one it is a key, quote removal makes it `}x`, and the
expansion *succeeds* — so this is not only a diagnostic difference. Note the
last two rows: `${!…}` indirects through the re-read reference, and `${#…}`
is the one shape already matching, because its name scan reports the parser's
body.

Nor is it confined to double quotes (row 2): the re-read is
`parameter_brace_expand`'s, which runs on every word.

**Fixed by** giving the word its second read. bash reads a word twice — the parser decides where it *ends*,
`expand_word_internal` re-derives everything else from the word's source — and
`wordscan.rs` already exists to model exactly that second read. Extend it with
the name scan (`extract_name` is already `string_extract` under `SX_VARNAME`),
and where the two reads disagree, re-parse the word's source under a
`ParseOpts` flag that carves a `${ … }` body with the subscript skip, then
expand *that* word. The swap has to happen at word level, not inside the
`${ … }`: the text the re-read swallows (`x"]` above) is in sibling parts, and
expanding those as well would print `HITx]}`. `Shell::begin_word` already
holds the word's source and is the one place every expansion passes through,
so it is where the replacement belongs — its seven callers each pick the
returned word over the parsed one.

That is what was built. `ParseOpts::reread` is not a shell option but a mode
of reading: with it, `read_dollar_brace_body` jumps from a `[` to the matching
`]` through the existing quote-aware `read_balanced`, so the jump crosses a
`}` — and a `"` — as bash's `skip_matched_pair` does. `Shell::reread_word`
re-parses the word's source under it from `Shell::begin_word` and hands the
result back for the seven callers to expand in place of the parsed word;
the control is the *same source* read the ordinary way, so the comparison
isolates the one rule that differs rather than measuring how faithfully
`unparse` spells a word back out.

Two details are bash's state machine rather than guesses, and both are
measured: only a subscript that **closes** is jumped over (`if (string[ni] ==
RBRACK)`), so `${a[}tail` is the plain bad substitution it always was; and the
jump needs `dolbrace_state == DOLBRACE_PARAM`, which `#` leaves (it is in the
first operator set the machine tests) and `!` does not — hence
`is_brace_name_so_far`.

**Covered by** the corpus case
`a-parameter-name-is-read-again-when-the-word-is-expanded.sh` and the unit test
`lexer::tests::the_re_read_jumps_from_a_subscript_to_its_bracket`.

**Severity (as filed):** low. The indexed-array shapes failed in both shells
with rc=1 and differed only in the diagnostic; the associative ones differed in
output. All of them need an unterminated subscript whose `]` appears later in
the same word.
