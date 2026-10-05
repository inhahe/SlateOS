### TD-OILS-A-COMSUB-THAT-NEVER-CLOSES-HIDES-THE-ERROR-INSIDE-IT. bash parses a `$( … )` body as it reads it; osh scans for the `)` first — 2026-08-06 — ✅ FIXED 2026-08-07, last two shapes 2026-08-14

**Where:** `userspace/oils/src/lexer.rs` — the command-substitution scanner
(the balanced-paren scan that raises ``unexpected EOF while looking for
matching `)'``), and `userspace/oils/src/parser.rs`, which parses the body
only after that scan has succeeded.

**What.** osh finds the closing `)` first and parses the body second. bash
does the opposite: `xparse_dolparen` (y.tab.c:6618) hands the *rest of the
input* to a recursive `parse_string (string, "command substitution", …)`
with `shell_eof_token = ')'`, so the body is parsed as it is read and the
missing `)` is only noticed if that inner parse survives all the way to the
end of input:

```c
  token_to_read = DOLPAREN;			/* let's trick the parser */
  nc = parse_string (string, "command substitution", sflags, (COMMAND **)NULL, &ep);
  …
  if (base[*indp] != ')' && (flags & SX_NOLONGJMP) == 0)
    {
      if ((flags & SX_NOERROR) == 0)
	parser_error (start_lineno, _("unexpected EOF while looking for matching `%c'"), ')');
```

The ordering is the whole bug. When the body is *both* syntactically bad and
unterminated, bash reports the inner error and osh reports the missing paren:

| script | bash | osh |
|---|---|---|
| `echo $(fi` | ``line 1: syntax error near unexpected token `fi'`` + echo | ``line 2: unexpected EOF while looking for matching `)'`` |
| `echo $(;` | ``line 1: syntax error near unexpected token `;'`` + echo | same EOF message |
| `echo $( ]]` | ``line 1: syntax error near unexpected token `]]'`` + echo | same EOF message |
| `echo $(fi)x$(` | ``line 1: syntax error near unexpected token `fi'`` + echo | ``line 2: unexpected EOF …`` |

The last row is the sharpest: the *first* substitution is the one bash
complains about even though it is the *second* one that is unterminated,
because bash parses substitutions left to right at parse time and never
reaches the second. Note also that bash's line is the line the body's error
is on, while osh's is the line past end of input.

Everything where the body closes is already byte-exact — `echo $(fi)`,
`$(;)`, `$(done)`, `$(then)`, `$(esac)`, `$(for)`, `$(a |)`, `$(!)` and
`$(()` all match — so this is purely about which of two errors wins, not
about parsing the body at all.

**The rule, measured (2026-08-07).** Once bash enters a `$(`, the
construct that *contained* it stops mattering: `parse_comsub` runs a whole
nested `yyparse` over the rest of the input with `shell_eof_token = ')'`,
so the enclosing quote is simply not open any more as far as that parse is
concerned. Every shape below falls out of that one sentence, and the
surprising rows are the ones where an enclosing `"` looks like it should
win and does not:

| script | bash reports | why |
|---|---|---|
| `echo $(fi` | `` near `fi' `` L1 | nested parse errors |
| `echo $(echo a` | `` EOF matching `)' `` L2 | nested parse reached EOF cleanly |
| `echo $(a \|` / `echo $(!` | `` EOF matching `)' `` L2 | nested parse ran *out*, so the `)` wins |
| `echo $(`⇥`fi` | `` near `fi' `` **L2** | the body is numbered physically |
| `echo $(fi) $(done` | `` near `fi' `` L1 | left to right; the second is never reached |
| `echo <(fi`, `echo >(fi` | `` near `fi' `` L1 | process substitution parses the same way |
| `echo ${x:-$(fi` | `` near `fi' `` L1 | the `${` never gets to miss its `}` |
| `echo $(( $(fi` | `` near `fi' `` L1 | nor does the arithmetic scan its `)` |
| `echo $(echo x; $(fi` | `` near `fi' `` L1 | recursion: the inner one bails in its turn |
| `echo "$(fi` | `` near `fi' `` L1 | the enclosing `"` is not consulted |
| `echo "$(fi)` | `` near `fi' `` L1 | **and not even when the body closes** |
| `echo "$(fi"` | `` EOF matching `"' `` L1 | the `"` is *inside the body*, and unclosed there |
| `echo "x$(fi"y"` | `` EOF matching `)' `` L2 | body is `fi"y"` — one quoted word, no error |
| ``echo `fi`` | ``EOF matching `` `' `` L1 | backticks are a matched pair, never parsed |
| `echo $(echo `` ` ``fi` | ``EOF matching `` `' `` L1 | …so the backtick inside the body dies first |
| `echo ${x`, `echo $((1+` | `` EOF matching `}'/`)' `` | no nested parse at all |

The last two rows are the boundary: only `$( )`, `<( )` and `>( )` get a
nested parse. `` ` ` ``, `${ }` and `$(( ))` are `parse_matched_pair`, and
osh already matches them.

**Fixed 2026-08-07.** The scan still finds the `)` first, but an error that
*is* the missing `)` now carries the body it ran out inside
(`lexer::SubstBail`: the text from just past the `(` to end of input, plus
the line the `(` sat on), and the parser — which owns the lexer-error-to-
`ParseError` conversion and holds the `ParseOpts` — parses it before
deciding (`parser::resolve_subst_bail` / `bail_body_error`). A body error
that is *not* `ParseError::is_incomplete` wins; one that is means the body
merely ran out, which is bash's `EOF_Reached` path, so the `)` message
stands. Three details that are not obvious and that the tests pin:

  * the body is parsed **plainly**, not through `parse_cmdsub_body`, whose
    synthetic trailing `)` would get `echo $(a |` blamed on a paren that is
    not there;
  * the bail is attached on the way *out* of each nested scan, so the
    **outermost** substitution wins and `echo $(fi; $(done` is `` near `fi' ``
    — the order bash blames in, its nested parse of the outer body being
    what reaches the inner `$(` at all;
  * within one body the halves are tried left to right — the tokens that did
    lex first, and only then the substitution the body itself ran out inside.

**Pinned by** `parser.rs::an_unterminated_substitutions_body_is_parsed_anyway`
and `tests/corpus/a-substitution-body-is-read-before-its-closing-paren-is-missed.sh`.

**The last two shapes, 2026-08-07 — ✅ fixed 2026-08-14.** Both wanted the
same thing: a `$( … )` body parsed at the moment the `$(` is *scanned*, closed
or not.

| script | bash | osh before |
|---|---|---|
| `echo "$(fi)` | `` near `fi' `` | ``EOF matching `"' `` |
| `echo $(fi)x$(` | `` near `fi' `` | ``EOF matching `)' `` L2 |

In both the substitution bash blames **closes**, so no `SubstBail` is raised
for it, and it sits inside a word the outer scan never finished — an unclosed
`"` in the first, an unclosed `$(` in the second — so the word yields no token
and the eager body parse the parser does per word never happens either. bash
has already run its nested `yyparse` over `fi` by then.

Both now agree, by the same carry-out-on-the-error device one construct further
out rather than by lifting the parse into the scan: a word that gives up leaves
its already-read eager bodies behind, and they ride out on the `LexError` to be
parsed by `parser::eager_body_error` ahead of whatever the word finally ran out
inside. See TD-OILS-AN-UNPARSEABLE-SUBSTITUTION-IN-AN-UNTERMINATED-DQUOTE-
LOSES-TO-THE-EOF for the design, the procsub and `Seg::Dq` details, and the
27-row corpus that pins it.

`echo $(fi) $(done` — previously listed here as a third shape — was fixed by
the `tokenize_deferred` change under
TD-OILS-A-LINE-THAT-FAILS-TO-LEX-IS-NOT-REPORTED-AS-A-LINE: the word `$(fi)`
does complete there, so keeping the failing line's tokens is enough for the
parser to reach it and raise `` near `fi' `` before the parked error is due.
The same change fixed `fi; $(done`, which was the proof that half of this
entry was never really about substitutions.

**The fix considered and not taken:** moving the eager body parse out of the
parser and into the scan — `read_balanced_body` already knows where each nested
`$( … )` / `<( … )` / `>( … )` body begins and ends, so it could parse each one
as it closes it, which is bash's own order. It was not needed: the scan has no
`ParseOpts`, and carrying the read bodies out on the error reaches the same
measured behaviour with a strictly smaller change along the line `SubstBail`
already established. `SubstBail` accordingly stays what it is — the *last*,
unterminated body, parsed on the way out because there is no `)` to close it
on — and `eager_bodies` covers every body that did close.
