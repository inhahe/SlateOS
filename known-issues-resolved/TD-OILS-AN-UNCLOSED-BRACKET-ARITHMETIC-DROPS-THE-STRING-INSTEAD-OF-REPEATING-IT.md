### TD-OILS-AN-UNCLOSED-BRACKET-ARITHMETIC-DROPS-THE-STRING-INSTEAD-OF-REPEATING-IT. `j='X$[9]Y$[ 1 + 2 Z'; "${j@P}"` gave `X9Y` where bash gives `X9YX$[9]Y$[ 1 + 2 Z` — 2026-08-09 — FIXED 2026-08-09

**Where:** `userspace/oils/src/interp.rs`, the `$[ … ]` arm of the
expansion-time scan — the ending taken when `extract_arithmetic_subst` finds no
closing `]`. osh contributed nothing and stopped; bash appends the **whole
original string** to whatever it had already accumulated.

**Reproduced** (all under `${x@P}`, i.e. a prompt expansion; the osh column is
what it gave *before* the fix):

```text
h='A$[ 1 + 2 B'        bash [AA$[ 1 + 2 B]              osh [A]
i='$[ 1 + 2'           bash [$[ 1 + 2]                  osh []
j='X$[9]Y$[ 1 + 2 Z'   bash [X9YX$[9]Y$[ 1 + 2 Z]       osh [X9Y]
```

The rule is plain in the third: the output is *everything expanded so far*
(`X9Y`, the closed `$[9]` included) followed by the original string entire —
not the remainder, and not the failing construct alone. Neither shell reports
anything; the difference is only what comes out.

**Why (traced, as the entry asked).** The guess above — that the shape came out
of `extract_delimited_string`'s failure return — was half right: that call is
where the NULL comes from (subst.c:1493-1506, under
`no_longjmp_on_fatal_error`), but the "original string entire" is not its doing.
It is `param_expand`'s own `case '['`, which is the only arm in the function that
answers a NULL extraction that way:

```c
case '[':		/*]*/
  t_index = zindex + 1;
  temp = extract_arithmetic_subst (string, &t_index);
  zindex = t_index;
  if (temp == 0)
    { temp = savestring (string);
      if (expanded_something) *expanded_something = 0;
      goto return0; }              /* subst.c:10650-10661 */
```

`return0:` is `*sindex = zindex;` (subst.c:10772), so the walk also ends there.
`savestring (string)` is the whole string the walk was handed, not the remainder
— which is exactly the third row above. The `$((` spelling has no such arm.

**Fix.** Three changes, all small once the arm was found:

1. `interp.rs`, `Shell::expand_unclosed` — the `Unclosed::BadSubst` prompt path
   grew a `close == ']'` case that returns the carried `text` (the string the
   walk was handed) instead of nothing.
2. `lexer.rs`, `read_dollar_brace_body` — the `$[` arm is now gated on
   `!self.here_text`. `extract_dollar_brace_string` has rows for `` ` ``, `$(`,
   `<(`, `"`, `'` and a `[` subscript and **none** for `$[` (subst.c:1881-1950),
   so which scan is reading the text decides whether an unclosed `$[` may
   swallow the `}`. A parser's `$[` row does; the expansion-time scan's does not,
   and `A${x:-$[ 1 + 2 }B` under `@P` is `A$[ 1 + 2 B`.
3. `lexer.rs`, `read_word_verbatim` — the `$` arm now routes a failure through
   `unclosed_seg`, as the two other value-scanning loops already did. bash has no
   operand re-lex at all: `parameter_brace_expand_word` (subst.c:7078) hands the
   operand back to `expand_word_internal`, so a construct left open in an operand
   fails at expansion, not at a scan. Without this the operand's `$[` was a hard
   lex error, the whole `@P` decode failed, and osh returned the value verbatim.

**Note:** the closed case was never affected (`$[9]` gives `9` in both); this was
only the no-closing-`]` ending. Corpus:
`an-unclosed-bracket-arithmetic-keeps-the-whole-string.sh` (17 `@P` shapes, plus
`PS4` and a here-document body, where nothing raises the flag and the extraction
reports and discards as it always did).
