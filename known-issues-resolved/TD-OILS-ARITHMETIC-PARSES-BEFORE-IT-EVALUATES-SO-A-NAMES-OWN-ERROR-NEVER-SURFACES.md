### TD-OILS-ARITHMETIC-PARSES-BEFORE-IT-EVALUATES-SO-A-NAMES-OWN-ERROR-NEVER-SURFACES. `x='1 + '; echo $(( 4 x ))` blames the leftover `x` where bash blames the `+` inside `x` — 2026-08-09 — ✅ FIXED 2026-08-09 (PART 1 SINGLE-PASS EVALUATOR, PART 2 ONE-TOKEN LOOKAHEAD)

**Where:** `userspace/oils/src/arith.rs`. The module *was* deliberately two-phase —
`fn parse(expr, vars: &dyn VarLookup) -> Expr` built an AST, then
`fn eval_expr(e, vars: &mut dyn VarLookup, depth)` walked it — and the
module doc said why: "The two-phase design is what makes assignment possible …
and `&&`/`||`/`?:` short-circuit so side effects only happen on the branch
actually taken." bash is single-phase: its recursive-descent parser *is* the
evaluator, and it suppresses the untaken branch with a `noeval` counter instead.
That half is now done — see "Part 1, as landed" below — and what is left is the
*other* half of the same sentence: bash's lexer runs one token **ahead** of the
parse, so it has already evaluated the token the parse is about to stop on.

**Reproduce.**

```sh
bad='1 + '
w=$' 3 bad '
echo "D [$(( $w ))]"        # the leftover arrives by splicing
echo "E [$(( 4 bad ))]"     # …and written in the source
echo "F [$(( 4 zzz ))]"     # zzz unset
```

| | bash 5.2.37 | osh |
|---|---|---|
| D | `1 + : syntax error: operand expected (error token is "+ ")` | `3 bad  : syntax error in expression (error token is "bad  ")` |
| E | `1 + : syntax error: operand expected (error token is "+ ")` | `4 bad : syntax error in expression (error token is "bad ")` |
| F | `4 zzz : syntax error in expression (error token is "zzz ")` | same ✅ |

Both abandon the `echo` and continue the script, so only the diagnostic differs.
The short-circuit shapes already agree — `$(( 0 ? bad : 5 ))`, `$(( 1 ? 5 : bad ))`
and `$(( 0 && bad ))` are `5`, `5`, `0` with no report on both.

**Why (read in the source, after measuring).** `readtok` evaluates a NAME **the
moment it lexes it**, not when the parser consumes it:

```c
      /* The tests for PREINC and PREDEC aren't strictly correct, but they
	 preserve old behavior if a construct like --x=9 is given. */
      if (lasttok == PREINC || lasttok == PREDEC || peektok != EQ)
        {
          lastlval = curlval;
	  tokval = expr_streval (tokstr, e, &curlval);   /* expr.c:1385-1390 */
        }
```

and `expr_streval` recurses into the value: `tval = (value && *value) ?
subexpr (value) : 0` (expr.c:1231). Lexing runs one token ahead of the parse, so
a *leftover* NAME has already been evaluated by the time `subexpr` reaches its
`if (curtok != 0) evalerror ("syntax error in expression")`. Whichever error
comes first wins, and the inner one always does.

Three sub-rules fall out, and F above measures the third:

* the suppression is `noeval`, checked at the very top of `expr_streval`
  ("If we are suppressing evaluation, just short-circuit here", expr.c:1157-1160)
  — so a NAME in an untaken branch is lexed but never recursed into;
* a NAME immediately followed by `=` is not evaluated either (`peektok != EQ`),
  which is what makes `x=1` an assignment rather than a read;
* an unset or **empty** value does not recurse at all (`value && *value`), so
  `$(( 4 zzz ))` reports the leftover — which is why osh gets F right by
  accident.

**Proper fix.** Interleave the two phases: make `arith.rs` a single-pass
recursive-descent *evaluator* with a `noeval` depth counter, the way expr.c is,
so a NAME resolves (and recurses through `str_to_val`) at the point it is lexed
and `&&`/`||`/`?:` raise `noeval` over the branch they skip instead of relying
on a not-yet-walked AST. That deletes the `Expr` AST and merges `parse`/
`eval_expr` — a real refactor of the file, but the two-phase split is the whole
cause and no smaller change reaches it. (Half-measures do not work: eagerly
resolving names during the current parse would evaluate untaken branches, which
is the very thing the AST was introduced to avoid.) — **done, part 1 below.**

Then make the walk *lex the token it stops on*, which is what D and E actually
turn on. See "Part 2, what is left".

**Part 1, as landed (2026-08-09).** `arith.rs` is now the single-pass evaluator:
`Expr`/`Lvalue`/`ResolvedLv`/`Sub` and `eval_expr` are gone, every `parse_*`
method returns a `V { n: i64, lv: Option<Lv> }` — bash's `tokval`/`curlval`
pair — and `AParser` carries `vars`, a `noeval` counter and the recursion
`depth`. The read happens in `AParser::reference`, at the point the name is
lexed, under exactly bash's condition (`forced || !eq_follows()`, and not at all
while `noeval > 0`).

Merging the phases settled five things that the AST had had wrong, none of them
previously logged, all now measured and covered by
`tests/corpus/arithmetic-evaluates-a-name-as-it-lexes-it.sh`:

* `**` is *not* suppressed. `exppower` (expr.c:960-978) has no `noeval` guard
  where `expmuldiv` does, so `$(( 0 ? 2**-1 : 5 ))` and `$(( 0 && 2**-1 ))` are
  "exponent less than 0" in bash. osh used to answer `5` and `0`.
* A prefix `++`/`--` **forces** the read a following `=` would suppress
  (`lasttok == PREINC || lasttok == PREDEC`, expr.c:1384-1391), and stores the
  increment before refusing the assignment: `x=5; (( --x=9 ))` leaves `x` at 4.
  osh left it at 5.
* An unread reference's subscript is evaluated by the *assignment*, i.e. after
  the right-hand side (`lind = curlval.ind`, expr.c:535-536; the unread case
  falls to `expr_bind_variable`, expr.c:600-604). `unset a q; (( a[q=1] = (q+9) ))`
  leaves `a[1]` = 9; osh left 10.
* A parenthesised group names no location (`lasttok = NUM`, `curlval`
  untouched), so `x=5; (( (x) = 3 ))` is "attempted assignment to non-variable"
  and `(( (x)++ ))` is "operand expected". osh assigned and incremented.
* A value that fails to evaluate is the expression the diagnostic is reported
  *from*: `bad='1 + '; $(( bad + ))` says `1 + : …` in bash, where osh said
  `bad + : …`.

One cost, taken deliberately: `RECURSION_LIMIT` dropped from 128 to 32. A
level's whole recursive-descent walk is now still on the stack while the value
it read is evaluated beneath it — measured at ~28 KiB per level in the debug
build, so 128 levels wanted 3-4 MiB and overflowed a `std` thread's 2 MiB
default. bash's own bound is 1024 (`MAX_EXPR_RECURSION_LEVEL`, expr.c:101) but
its frames are far smaller. Nothing legitimate goes past a handful.

**Part 2, as landed (2026-08-09).** D and E are the *lookahead*, and the note
that used to end this entry — "the same one-token lookahead osh's parser already
has" — was wrong. osh's parser peeked at *characters* to choose a production; it
never lexed the token it declined, so a NAME sitting where an operator was
expected was never evaluated. bash's `readtok` runs after every consumed token,
so by the time `subexpr` reaches `if (curtok != 0) evalerror ("syntax error in
expression")` the leftover has already been lexed — and lexing it is not free.

Landed as prescribed: a `fn lex_here(&mut self) -> Result<(), ArithError>` at the
one site `parse_binary`'s loop breaks because the next token is not a binary
operator. Every operand in the grammar flows through that loop (`parse_comma` →
`parse_assign` → `parse_ternary` → `parse_binary`), so that break is the one
place a leftover can be reached. `lex_here` skips whitespace, lexes exactly one
token there, and restores the cursor; a `lexed_at: Option<usize>` guard makes it
lex the once, as bash's `curtok` is lexed once and then held — without it the
leftover would be re-evaluated at every grammar level that unwinds past it. The
lexer proper is now `fn lex_token(&mut self) -> Result<(Tk, Option<Lv>),
ArithError>`, which names the error token (`mark_tok`, bash's `lasttp = tp = cp
- 1`) for every token *but* end-of-input — bash returns before that assignment
(expr.c:1326-1333), which is what makes `$(( 2 ** -1 ))` blame the `1`.

Four things the lookahead does, all of them now reproduced and covered by
`tests/corpus/arithmetic-lexes-one-token-past-the-parse-and-lexing-is-not-free.sh`:

* a **NAME** is evaluated (`$(( 4 bad ))` is `1 + : … operand expected`), which
  recurses and answers to the recursion limit, obeys `set -u`, and performs
  whatever assignments the value contains — `x='y=5'; (( 4 x ))` leaves `y` at 5
  even though the expression failed;
* a **number** is converted, and can fail to convert (`$(( 4 5x ))` is `value
  too great for base`) — see below;
* a **subscript** is scanned, and an indexed one evaluated (`$(( 4 zzz[1+] ))`);
* a character the lexer has **no token for** is refused where it is met
  (`$(( 4 @ ))` is `syntax error: invalid arithmetic operator`).

`noeval` suppresses only the *read* (`expr_streval` short-circuits at
expr.c:1157-1160), so a leftover in an untaken `&&`/`||`/`?:` branch is still
lexed there — `$(( 0 && 4 5x ))` fails, `$(( 0 && 4 bad ))` does not — and the
error token still moves, which is what fixes the `$(( 0 ? 2 bad : 3 ))` row.

**Two layers measurement added that the prescription above did not predict:**

* **A NAME's own peek for `=` is a whole token, lexed with evaluation
  suppressed** — not the character peek `eq_follows` was. bash does `SAVETOK
  (&ec); noeval = 1; curtok = STR; readtok (); peektok = curtok; RESTORETOK
  (&ec);` before deciding whether to read the name at all (expr.c:1375-1391).
  `noeval` stops the *read* and nothing else, so that peek can still fail on a
  literal or a stray character, and its failure **precedes the name's own read**:
  `set -u; echo $(( zzz 5x ))` is `value too great for base`, not `unbound
  variable`. Suppression applies inside the peek too, so a run of names is walked
  to its end (`x y z 5x` fails on the literal three names later). osh implements
  the walk as a **loop** (`peek_chain`), not a recursion, because a suppressed
  pass reads nothing and does nothing on the way back out — recursing overflowed
  the stack at 50 000 chained names where bash survives (bash itself dies
  silently around 20 000; osh matches it exactly at 5 000 and 10 000). And
  `SAVETOK`/`RESTORETOK` carry `lasttp`, so a peek that succeeds leaves no
  trace: neither the cursor nor the error-token position moves.
* **`++`/`--` in operator position is a prefix increment, not two operators.**
  When the left operand cannot take a postfix, bash's lexer looks *forward*
  (expr.c:1466-1487): a `legal_variable_starter` after it makes the token
  PREINC/PREDEC — which, sitting where a binary operator was expected, ends the
  expression — and anything else does `cp--`, splitting it into a binary sign and
  a unary one. So `$(( 4 ++ 5 ))` is 9 while `$(( 4 ++ y ))` is a syntax error.

Part 3 of the old entry — a leftover *number* — was recorded as "a **separate**
divergence … which want their own entry once part 2 is in". The measurement
disagreed: it is the same `readtok`, the same lookahead, and the same `lex_here`
fixes it, so there is no second entry. `$(( 4 5x ))`, `$(( 4 0x8#1 ))`,
`$(( 4 099 ))` and `$(( 4 2#12 ))` are all byte-identical now.

One detail of those rows worth keeping: bash's echo and error token both stop at
the end of the *lexeme*, with no trailing blank, where every other diagnostic in
this entry keeps the rest of the string. That is not a rule but an accident of
`readtok`'s digit branch, which NUL-terminates the lexeme *in place* across the
`strlong` call (expr.c:1398-1409):

```c
      else if (DIGIT(c))
	{
	  while (ISALNUM (c) || c == '#' || c == '@' || c == '_')
	    c = *cp++;
	  c = *--cp;
	  *cp = '\0';
	  tokval = strlong (tp);
	  *cp = c;
```

`strlong` is where "invalid number"/"value too great for base" are raised, so
`evalerror` — which echoes `expression` and prints `lasttp` (expr.c:1517-1530) —
observes the string while that temporary NUL is in it, and both are truncated at
the lexeme end. osh already reproduced this for a number in *operand* position;
it now does so in lookahead position too, by the same code path.
