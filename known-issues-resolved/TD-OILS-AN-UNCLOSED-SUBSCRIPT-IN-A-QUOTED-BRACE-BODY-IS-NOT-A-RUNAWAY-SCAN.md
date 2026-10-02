### TD-OILS-AN-UNCLOSED-SUBSCRIPT-IN-A-QUOTED-BRACE-BODY-IS-NOT-A-RUNAWAY-SCAN. `echo "${a[}"` names the wrong subject — 2026-08-08 — ✅ FIXED 2026-08-08

**Where:** was `userspace/oils/src/lexer.rs`, `read_dollar_brace_body`; is now
`userspace/oils/src/wordscan.rs`, a new module, called from
`Shell::begin_word` (`userspace/oils/src/interp.rs`). bash's counterpart is
`extract_dollar_brace_string` (subst.c:1813), reached from the double-quote
extent scanner (`string_extract_double_quoted`, subst.c:963).

**What.** Inside double quotes bash scans the `${…}` twice: once to find the
extent of the quoted word, once to expand it. The extent scan skips a subscript
wholesale —

```c
      if (c == LBRACK && dolbrace_state == DOLBRACE_PARAM)   /* subst.c:1943 */
	{
	  si = skipsubscript (string, i, 0);
	  CHECK_STRING_OVERRUN (i, si, slen, c);
	  if (string[si] == RBRACK)
	    c = string[i = si];
	}
```

— and `skipsubscript` looks for the `]` in the **whole word**, quotes and all,
not just in the brace body. With no `]` anywhere it runs to the end, and
`CHECK_STRING_OVERRUN` (subst.c:135) turns that into `ch = 0; break`, which lands
on the `if (c == 0 && nesting_level)` at subst.c:1975 —
`bad substitution: no closing `}' in %s`, naming the entire word:

```text
                    bash                                            osh
echo "${a[}"        bad substitution: no closing `}' in "${a[}"      ${a[}: bad substitution   rc=1 both
echo "${a[}"tail    bad substitution: no closing `}' in "${a[}"tail  ${a[}: bad substitution   rc=1 both
echo "pre${a[}post" no closing `}' in "pre${a[}post"                 ${a[}: bad substitution   rc=1 both
echo "${a[0}"       bad substitution: no closing `}' in "${a[0}"     ${a[0}: bad substitution  rc=1 both
echo "${[}x}"       bad substitution: no closing `}' in "${[}x}"     (empty), then `x}'        rc=1 / rc=0
echo ${a[}tail      ${a[}tail: bad substitution                      same                      rc=1 both
```

Three things the rows pin down:

* **It only happens inside double quotes.** An unquoted `${a[}` never meets the
  extent scan — `expand_word_internal` goes straight to `param_expand` — so it
  is an ordinary bad substitution and osh already agrees (last row).
* **The subject is the whole word**, not the `${…}`: `"${a[}"tail` and
  `"pre${a[}post"` name every byte of it, including the quotes.
* **`${#[}` is exempt** and already agrees, because the leading `#` moves
  `dolbrace_state` to `DOLBRACE_OP` (subst.c:1998) before the `[` is reached, and
  the skip is gated on `DOLBRACE_PARAM`.

**Fixed.** Not in the lexer. The disagreement is between bash's two *reads* of
a word, and osh's lexer is the first of them — giving it the subscript skip
would have moved the divergence rather than removed it, because the parser must
still close `"${a[}"` at the `}` (the word ends where bash's parser says it
ends). So the second read is now modelled where bash makes it: at expansion
time, over the word's source text.

`userspace/oils/src/wordscan.rs` is that read — a set of pure byte scanners
mirroring `expand_word_internal`'s walk (subst.c:9989),
`string_extract_double_quoted` (subst.c:963), `extract_dollar_brace_string`
(subst.c:1813), `skip_double_quoted` (subst.c:1020), `string_extract` with
`SX_VARNAME` (subst.c:795) and `skip_matched_pair` (subst.c:2085). Its one
entry point, `unclosed_brace`, answers "does the extent pass meet a `${` it
cannot close, and if so what string does the complaint name?".
`Shell::begin_word` asks it and, on a hit, calls the new
`Shell::dq_unclosed_brace`, alongside the `$(( … ))`-comment verdict already
raised there.

Four things came with it, each measured before it was written:

* **The complaint is the scanner's, so it precedes the word's own side
  effects.** `echo "$(echo ran >&2)x${a[}"` prints no `ran` in bash, and now
  none in osh — the check runs in `begin_word`, before any part is expanded.
* **It is errexit-only.** bash raises it with a bare
  `exp_jump_to_top_level (DISCARD)` (subst.c:1978) rather than by returning an
  error word, so posix mode's "an expansion error ends the shell" hook never
  sees it: `set -o posix; echo "${a[}"` complains and the next line runs, where
  the plain bad substitution ends the shell. `FatalWhen::ErrexitOnly`.
* **The name is the string the innermost `expand_word_internal` was handed** —
  the whole word for a fault the extent scan found (`"a${a[}b"`), the run's
  contents without quotes for one found a level in (`"${x:-${a[}}"` names
  `${x:-${a[}}`). The scanner recurses on the run it extracts and carries the
  name in its `Err`, so this falls out rather than being special-cased.
* **It subsumes the `$[` divergence** (TD-OILS-A-DOUBLE-QUOTED-DOLLAR-BRACKET-IS-NOT-SKIPPED-BY-THE-QUOTE-EXTRACTOR),
  which was the same fact from the other side: `$[` is not a construct either
  double-quote scanner knows, so the `${` after it is met.

The recorded `"${[}x}"` row above was stale by the time the fix was written —
osh had come to agree with bash's status on it since; re-measured, the whole
table is now byte-identical.

**Pinned by:** six tests in `wordscan.rs` and
`tests/corpus/an-unclosed-subscript-in-a-quoted-brace-body-is-a-runaway-scan.sh`.
