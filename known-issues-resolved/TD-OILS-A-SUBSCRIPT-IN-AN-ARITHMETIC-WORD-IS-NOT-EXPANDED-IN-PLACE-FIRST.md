### TD-OILS-A-SUBSCRIPT-IN-AN-ARITHMETIC-WORD-IS-NOT-EXPANDED-IN-PLACE-FIRST. `[[ -v "a['1']" ]]` and `(( ~q['1'] ))` report where bash accepts — 2026-08-10 — ✅ FIXED 2026-08-10

**Where:** `userspace/oils/src/interp.rs` — `Shell::element_is_set` (the `[[ -v ]]`
operand) and `Shell::expand_to_arith_string` / `Shell::arith_string_parts`
(`(( … ))`, `$(( … ))`, a subscript, a substring bound). osh always hands a
subscript's text to the *arithmetic* reading, in which a `'` is not a quote.
bash does that too — but only when the string reached arithmetic **without** a
full word expansion. When there was one, a `[ … ]` in the word is expanded in
place first, with ordinary quoting where a `'` *is* a quote, and the result is
backslash-escaped so the arithmetic's own read cannot expand it a second time.

**Reproduce.**

```sh
a=(A B C D E); q=(9 8 7 6 5); s='$(echo 1)'
[[ -v "a['1']" ]];           echo "1 rc=$?"
[[ -v "a['\$(echo 1)']" ]];  echo "2 rc=$?"
[[ -v "a['x y']" ]];         echo "3 rc=$?"
(( a["'"$s"'"] ));           echo "4 rc=$?"
(( ~q['1'] ));               echo "5 rc=$?"
(( q['1'] + $# ));           echo "6 rc=$?"
```

| | bash 5.2.37 | osh |
|---|---|---|
| 1 | `rc=0`, no diagnostic | `'1': syntax error: operand expected (error token is "'1'")` |
| 2 | `\$(echo 1): syntax error: operand expected (error token is "\$(echo 1)")` | `'$(echo 1)': …` |
| 3 | `x y: syntax error in expression (error token is "y")` | `'x y': syntax error: operand expected …` |
| 4 | `\'$(echo 1)\': syntax error: operand expected …` | `'$(echo 1)': …` |
| 5 | `rc=0` — the subscript became `1`, `q[1]` is `8`, `~8` is nonzero | `'1': syntax error …` |
| 6 | `rc=0` | `'1': syntax error …` |

**These already match, and they are what pins the gate down.** Drop the `~` from
row 5 and the `$#` from row 6 and bash reports `'1'` exactly as osh does; so do
`(( a['1'] ))`, `(( a["1"] ))`, `let "a['1']"`, `echo $(( a['1'] ))`,
`echo "${a[b['1']]}"`, `${z:b['1']:1}`, `a['1']=Q` and `declare -a d; d['1']=S`.
A backslash in the string is right too: `(( a[\~] ))` reports `\~`, `(( a[\'] ))`
reports `\'`, while `(( a[\$] ))` reports a bare `$` and `` (( a[\`] )) `` a bare
`` ` ``.

**What bash does.** An arithmetic string goes to `expand_arith_string (s,
Q_DOUBLE_QUOTES|Q_ARITH)` → `expand_string_if_necessary` (subst.c:4018-4079),
which first scans for a character that could start an expansion:

```c
#define ARITH_EXP_CHAR(s) (s == '$' || s == '`' || s == CTLESC || s == '~')
                                                        /* subst.c:3824 */
```

Finding none, it takes `string_quote_removal (string, quoted)` (subst.c:4073-4074)
— a plain pass with `Q_DOUBLE_QUOTES` still set, so a `'` stays a character and
nothing else happens. That is the case osh already matches, and it is most of
them. Finding one, it runs the whole of `call_expand_word_internal (…, Q_ARITH)`
instead (subst.c:4052) — and *that* has a `[` row:

```c
	case '[':		/*]*/
	  if ((quoted & Q_ARITH) == 0 || shell_compatibility_level <= 51)
	    { … goto add_character; }
	  else
	    {
	      temp = expand_array_subscript (string, &sindex, quoted, word->flags);
	      goto add_string;
	    }                                        /* subst.c:11103-11115 */
```

`expand_array_subscript` (subst.c:10836-10894) is where the two properties come
from:

```c
  exp = substring (string, si+1, ni);
  t = expand_subscript_string (exp, quoted & ~(Q_ARITH|Q_DOUBLE_QUOTES));
  free (exp);
  exp = t ? sh_backslash_quote (t, abstab, 0) : savestring ("");
                                                  /* subst.c:10878-10881 */
```

The subscript is expanded with `Q_ARITH` and `Q_DOUBLE_QUOTES` **masked off** —
quoting 0 — so a `'` there *is* a quote and comes off, which is rows 1-5. And the
result is `sh_backslash_quote`d against `abstab`, holding `[`, `]`, `$`,
`` ` ``, `~`, `\`, `'` and `"` (subst.c:10848-10857) — the characters that would
start *another* expansion — so the evaluator's own second read of the subscript
cannot expand it again. `[[ -v "a[\$s]" ]]` shows that guard directly: bash
reports `$s`, unexpanded, where osh expands it.

`[[ -v ]]` is the one place with no gate at all: `cond_expand_word (cond->left->op,
varop ? 3 : 0)` (execute_cmd.c:3913-3919, `varop` is `-v` alone) reaches
`call_expand_word_internal (w, Q_ARITH)` unconditionally (subst.c:4117-4131), so
rows 1-3 take the `[` row however plain the operand is. `test -v` is *not* `[[ -v
]]` — it takes an ordinary word and reports `'1'`, which osh matches.

**The fix — done.** `WordPart::ArithSubscript(Vec<WordPart>)` (`ast.rs`) is a
part the parser never builds: the expander splices it into a word it is about to
expand, and its parts are the subscript's *source* re-read as an ordinary bare
word. `Shell::arith_subscript_parts` does the splicing — it finds each top-level
bracket run on the word's own source with `wordscan::skip_subscript` (new, a
`skipsubscript` wrapper over the existing `skip_matched`), recursing into
`DoubleQuoted` because `Q_ARITH` survives that recursion (subst.c:11426) — and
`Shell::expand_arith_subscript` expands one, with `no_split_star` set as
`expand_subscript_string` sets `expand_no_split_dollar_star`
(subst.c:10801-10812), and wraps the result in `backslash_quote_subscript`
(`SUBSCRIPT_QUOTE_CHARS` = bash's `abstab`) and its two brackets. Keeping it a
*part* rather than expanding eagerly is what preserves ordering: bash runs `f`
before `g` in `[[ -v "$(f)[$(g)]" ]]`, which a pre-pass would reverse.

Three callers:

* `Shell::expand_cond_arith_string`, reached from `cond_unary` for `-v` alone —
  ungated, as `cond_expand_word (…, 3)` is;
* `Shell::expand_to_arith_string_inner`, gated on `arith_exp_char` over the
  word's source;
* `Shell::expand_arith_params_inner`, the raw-source scanner that `(( … ))`,
  `$(( … ))`, `$[ … ]` and `for (( … ))` actually use — gated the same way, over
  the whole expression, so a `~` at the far end opens the near end's subscript.

The **second reading** is now modelled too, which is what makes the backslashes
come out right: `VarLookup::expand_index_subscript` /
`VarLookup::expand_assoc_subscript` (`arith.rs`), called from `AParser::eval_sub`
and from the `Lv::Assoc` arm of `lex_reference`, are `array_expand_index`'s
`expand_arith_string (exp, Q_DOUBLE_QUOTES|Q_ARITH|Q_ARRAYSUB)`
(arrayfunc.c:1358) and `array_value_internal`'s `expand_subscript_string (t, 0)`
(arrayfunc.c:1593). `Shell` answers them with `expand_arith_params` and with an
ordinary word expansion. The blame names the *expanded* text, so `x='$y'; ((
a[$x] ))` reports `$y` and `(( ~a[b[1]] ))` reports `b\[1\]`.

Two bracket counters had to become `skipsubscript`es for any of it to be
readable back: `AParser::lex_reference` (bash's `expr_skipsubscript`,
expr.c:1348-1358) and `Shell::split_array_ref` (bash's `valid_array_reference`).
Without them `(( ~a[\[] ))` and `k='['; [[ -v "m[$k]" ]]` both give up on
subscripts the expansion had just backslash-quoted. That also fixed
`(( a[1\]] ))`, which used to be `((: a[1\]] : …` and is now bash's `1\]`.

Corpus:
`tests/corpus/a-subscript-in-an-arithmetic-word-is-expanded-in-place-first.sh`
(74 rows: the ungated `[[ -v ]]` path including the `$(f)`/`$(g)` ordering row,
both sides of the gate, the backslash asymmetry, the assoc second reading, the
name check, and the already-agreeing rows that pin the gate down).

**Not caused by the `' … '` work of 2026-08-10** (TD-OILS-A-SINGLE-QUOTED-RUN-IN-
A-BARE-SUB-WORD-OF-A-BRACE-IS-A-QUOTE); it was measured during it, and every row
above diverged identically before and after.
