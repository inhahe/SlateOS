### TD-OILS-AN-UNTERMINATED-BRACE-LEFT-BY-AN-ARITHMETIC-SCAN-IS-NOT-A-BAD-SUBSTITUTION. `echo $[ 1 + ${x:-]} ]` — 2026-08-07 — ✅ FIXED 2026-08-07

**Where:** `userspace/oils/src/interp.rs` — the `'{'` arm of
`Shell::expand_arith_params` (~line 28620) and the helpers now sitting before
`Shell::arith_sub`.

**What was wrong.** Neither `$((` nor `$[` skips a `${ … }` when it looks for
its end (that wants `P_ARRAYSUB|P_DOLBRACE`, parse.y:3929), so a `]` or `)`
*inside* a brace ends the scan and the text handed on carries an unterminated
brace. bash expands that text as a word before evaluating it, so the failure is
the word expansion's; osh reached the arithmetic parser with the `${x:-`
silently dropped and complained about the hole it left:

```text
bash: line 1: bad substitution: no closing `}' in  1 + ${x:-              rc=1
osh : line 1: 1 + : syntax error: operand expected (error token is "+ ")  rc=1
```

**The rule, which is not about the brace at all.** bash has *two* complaints
here and both come out of `parameter_brace_expand` (subst.c:9539), which reads
the name before it ever goes looking for the `}`:

* `string_extract` (subst.c:795) ends the name only at one of `#%^,~:-=?+/@}`
  — not at a space, and not at any other character a name may not hold — and it
  steps over a `\c` pair and a balanced `[…]`. A name that runs to the end of
  the text finishes with `c == 0`, which the `switch` at subst.c:10034 sends to
  `bad_substitution`. So `${x ` and `${a[x:-` are plain bad substitutions.
* `valid_brace_expansion_word` (subst.c:9814) then refuses anything that is not
  an identifier, an all-digit positional, a one-character special parameter or a
  well-formed `a[…]` reference — also before the brace is missed.
* The one-character special parameters that are *also* operator characters are
  re-scanned (`VALID_SPECIAL_LENGTH_PARAM`, subst.c:9605; `VALID_INDIR_PARAM`,
  subst.c:117 — which holds `# ? @ *` and deliberately not `-`), and `@` is made
  a name of its own at subst.c:9577. That is why `${@ ` is the missing brace and
  `${@))` is a bad substitution, and why `${!-` differs from `${!#`.
* A name behind a `!` is *resolved* first (`parameter_brace_expand_indir`,
  subst.c:7621), so an unset pointer is `invalid indirect expansion` and a
  pointer holding a non-name is `invalid variable name` — in the missing brace's
  place. The target's **value** is not read, which is why
  `set -u; y=z; echo $(( ${!y- ))` is the missing brace and not `z: unbound`.

Only a valid name stopped by an operator reaches `extract_dollar_brace_string`
(subst.c:9910), and only that call says ``no closing `}'`` — naming the whole
arithmetic string, because that is the string it was handed.

**Fixed by** `Shell::unterminated_brace_kind`, which reproduces that front half
and returns `BadSub` / `NoClosing` / `Indir(name)`; `Shell::brace_name_end` and
`Shell::brace_subscript_end` for `string_extract`'s stepping;
`Shell::valid_brace_expansion_word`; `Shell::arith_unclosed_brace` for the
missing-brace diagnostic (errexit-only, since bash raises it with a bare
`exp_jump_to_top_level (DISCARD)` that posix mode's hook never sees); and
`Shell::arith_indir_resolves`, which performs the indirection through the
existing `indirect_pointer_value`.

Corpus case:
`tests/corpus/an-unterminated-brace-in-an-arithmetic-string-is-named-by-its-own-rule.sh`.
Two neighbours found while measuring this are logged separately:
TD-OILS-A-DOUBLE-QUOTED-DOLLAR-BRACKET-IS-NOT-SKIPPED-BY-THE-QUOTE-EXTRACTOR and
TD-OILS-AN-ARITHMETIC-STRING-NAMES-ITS-COMMAND-SUBSTITUTION-AS-WRITTEN.
