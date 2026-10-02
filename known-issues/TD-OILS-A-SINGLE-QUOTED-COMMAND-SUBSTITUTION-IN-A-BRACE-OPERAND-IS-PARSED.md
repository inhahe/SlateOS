### TD-OILS-A-SINGLE-QUOTED-COMMAND-SUBSTITUTION-IN-A-BRACE-OPERAND-IS-PARSED. `"${z:-'$(fi⏎q)'}"` is a parse error where bash defers it to expansion — 2026-08-10

**Where:** `userspace/oils/src/parser.rs` — the lexing of a `${ … }` met inside a
double-quoted string, where the operand's `' … '` is being treated as *not*
quoting the `$( … )` inside it.

**What is wrong.** Inside `${ … }`, bash's `parse_matched_pair` tracks
`dolbrace_state`, and once the state is `DOLBRACE_QUOTE`/`DOLBRACE_WORD` a `'`
opens a single-quoted run that the parser passes over whole — it never hands
what is inside to `xparse_dolparen`. The `$( … )` in there is only met later, at
expansion time, and by then it is a *string* being scanned rather than source
being parsed. So bash's diagnostic comes from the expansion (`command
substitution: line N:`) and the failure is an `exp_jump_to_top_level (DISCARD)`
— the one command is dropped and the shell carries on.

osh parses it at word-parse time and dies:

```sh
unset z
echo "A: start"
echo "[${z:-'$(fi
q)'}]"
echo "B: after rc=$?"
```

| | bash 5.2.37 | osh |
|---|---|---|
| reports | `` command substitution: line 5: syntax error near unexpected token `fi' `` | `` line 3: syntax error near unexpected token `fi' `` + `` line 3: `echo "[${z:-'$(fi' `` |
| then | `A: start` … `B: after rc=1` | nothing after `A: start` |
| exit | 0 | 2 |

Two things wrong at once: the error is raised a whole phase early (so it is a
*script* syntax error rather than a discarded command), and the line it names
and the text it echoes are the physical line rather than the substitution's own.

**Not** merely cosmetic — the shell aborts where bash continues.

**Controls that already agree:** a single-quoted substitution that *parses* is
handled correctly at every level, because bash and osh end up expanding it the
same way (`extract_dollar_brace_string` skips the `' … '` at scan time, but
`expand_word_internal` is past the point where a `'` quotes anything, so the
substitution runs and the quotes stay as text):

```sh
unset z
echo "1[${z:-'$(echo Q)'}]"      # both: 1['Q']
echo "2[${z:-'a b'}]"            # both: 2['a b']
echo "3[${z:-'$(echo Q
)'}]"                            # both: 3['Q']
```

So the divergence is confined to a body that does not parse.

**Pre-existing** — found 2026-08-10 while measuring read 2 of
TD-OILS-A-BRACE-OPERAND-IS-SCANNED-AGAIN-BEFORE-IT-IS-EXPANDED, and untouched by
that work (it is a parse-time failure; the extent machinery never runs).

**What the proper fix looks like.** The `${ … }` lexer has to treat a `'` in the
operand as opening an unparsed run, exactly as `parse_matched_pair` does, so the
`$( … )` inside it is carried as text and only read when an expansion asks for
it. Fixing it also unblocks the read-2 corpus row that this shape is the
cleanest probe for: bash's `string_extract_double_quoted` (subst.c:966-985) has
no single-quote rule at all, so read 2 is the *only* reader that ever looks
inside it.

#### Which reader it actually is — `brace_gobbler`, not an expansion — 2026-08-10

The reading above has the *phase* right and the *reader* wrong, and the reader
is what decides the fix's shape. Measured first, then found in the C source.

**The measurements that rule out every expansion.** With `z` unset, bash reports
for **all five** of these, identically, and prints nothing else for any of them:

```sh
echo "R [${z:+'$(fi)'}]"      # ${…} branch never taken
echo "R [${z'$(fi)'}]"        # not a substitution at all — a bad one
echo "R [${z//x/'$(fi)'}]"    # a replacement
echo "R [${z#'$(fi)'}]"       # a pattern
echo "R [${a['$(fi)']}]"      # a subscript
```

Every one gives `command substitution: line 3: syntax error near unexpected
token `fi'` and `` command substitution: line 3: `fi)'}]"' ``, then carries on to
the next command with `$?` = 1. So the reader is not the operand's expansion
(`:+` unset never expands one), not the `${ … }` evaluation (the second line
would be `bad substitution` otherwise, and osh does say that), and not the
subscript's arithmetic. It also runs *before* all three of them: its diagnostic
beats every verdict they reach.

It is nevertheless execution-time, not parse-time: a `false && echo "…"` on the
same line reports nothing, and neither does the same word in a function body
that is never called.

**The decisive experiment.** `set +B` suppresses it outright:

```sh
unset z
set +B; echo "braces off [${z:+'$(fi)'}]"   # bash: `braces off []`
set -B; echo "braces on  [${z:+'$(fi)'}]"   # bash: the two error lines, no output
```

So the reader is **brace expansion** — `brace_gobbler`, braces.c:646-683. It
scans the *raw word text* with a quoting model much cruder than the parser's:

```c
      /* If compiling for the shell, treat ${...} like \{...} */
      if (c == '$' && text[i+1] == '{' && quoted != '\'')		/* } */
	{ pass_next = 1; i++; if (quoted == 0) level++; continue; }
      if (quoted)
	{
	  if (c == quoted) quoted = 0;
	  /* The shell allows quoted command substitutions */
	  if (quoted == '"' && c == '$' && text[i+1] == '(')	/*)*/
	    goto comsub;
	  ADVANCE_CHAR (text, tlen, i); continue;
	}
      if (c == '"' || c == '\'' || c == '`') { quoted = c; i++; continue; }
      if ((c == '$' || c == '<' || c == '>') && text[i+1] == '(')
	{ comsub: si = i + 2; t = extract_command_subst (text, &si, 0); … }
```

A `${` does not open a state of its own — it is passed over like `\{`. So inside
`" … "` the state stays `'"'` for the whole `${ … }` body, a `'` there is not a
quote at all, and every `$(` in it goes to `extract_command_subst` →
`xparse_dolparen`. That is the parse whose failure is reported, and its
`jump_to_top_level (DISCARD)` is what drops the command.

**Why exactly this shape and no other.** Line the two readers up:

| the `$( … )` sits in | bash's parser (`parse_matched_pair`) | `brace_gobbler` |
|---|---|---|
| `' … '` at top level | skipped | `quoted == '\''` — skipped |
| `' … '` inside `" … "` | read (a `'` is not a quote in double quotes) | read |
| `' … '` inside an **unquoted** `${ … }` | skipped (grouping construct, parse.y:3840-3846) | `quoted == '\''` — skipped |
| `' … '` inside a **double-quoted** `${ … }` | **skipped** | **read** ← the only disagreement |
| `\$( … )` | skipped | `pass_next` — skipped |

So `brace_gobbler` parses exactly one thing the parser did not: a `$( … )` the
`${ … }` body's own quoting hid. Everything else it re-parses was parsed already
and parses again silently.

**A second instance of the same shape:** `$' … '`. bash's parser translates it
where it stands (parse.y:3852-3893) and splices the *result* into the body
without re-reading it, so `"${z:-$'$(fi)'}"` stores the word `"[${z:-$(fi)}]"`
and the gobbler meets a bare `$( … )` — the echoed remainder is `` `fi)}]"' ``,
with no `'`, which is how the splice shows through. osh dies at parse time here
too.

**What `declare -f` shows, with nothing failing at all.** The same disagreement
is visible without any error, and this is the cheapest regression probe:

```sh
f1() { echo "[${z:-'$( (echo 2) )'}]"; }
declare -f f1
```

bash prints the operand **as written** — `'$( (echo 2) )'` — because its parser
never read that substitution and so never re-printed it. osh prints
`'$( ( echo 2 ))'`, its own re-print. Measured across nine body shapes
(`:-`, `//`, `#`, `%`, `:off:len`, `[sub]`, `^`, `:+`, `/`), bash keeps all nine
verbatim and osh matches on seven — only the `:-`/`:+`-class **operand** path
(`operand_from_source` → `lex_operand_in_dquote`) re-prints, which is the same
path that dies.

**So the fix is two pieces, and the first is a prerequisite for the second:**

1. **Stop reading what the parser skipped.** `operand_from_source`'s
   double-quoted path must carry a `' … '` run's `$( … )` as
   [`CmdSubBody::Unread`] rather than parsing it, and must not re-print the run
   — `declare -f` has to give the source back. Same for the text a `$' … '`
   translation spliced in. This alone stops the fatal abort and fixes the
   `declare -f` rows, but leaves osh *silent* where bash reports.
2. **Add the gobble pre-scan.** At `Shell::expand_braces_opt` (interp.rs:24385)
   — which is already the single `set -B` gate — the substitutions the parser
   skipped have to be parsed, in word order, before anything in the word is
   expanded, with a failure reported as `command substitution: line N:` and the
   command discarded (`$?` = 1). The line is the runtime line plus the error's
   offset inside the body, and the text echoed back is the substitution's
   remainder *of the whole word*, closing quote included — the same remainder
   [`CmdSubBody::Unread::tail`] already carries, since bash hands
   `extract_command_subst` the raw word from just past the `$(`.

   The pre-scan does **not** need to be a general port of `brace_gobbler`: by the
   table above it would only ever re-parse substitutions the parser already
   parsed, which is unobservable, so collecting the skipped ones where they are
   skipped is exactly equivalent and does not risk re-parsing a re-print.

#### Both pieces are in — ⏳ MOSTLY FIXED 2026-08-10

Piece 1 is `Lexer::read_word_verbatim`'s second `'` arm
(`tests/corpus/a-single-quoted-run-in-a-double-quoted-brace-operand-is-an-extent-not-a-quote.sh`);
piece 2 is `Shell::gobble_scan` and `crate::unparse::gobbler_word`
(`tests/corpus/the-brace-scanner-reads-the-command-substitutions-a-single-quote-hid.sh`).
The abort is gone, `declare -f` gives the source back, and the diagnostic, its
line, its whole-word remainder, its `set -B` gate, its stop-at-the-first-failure
and its reach across word kinds all match bash 5.2.37.

The walk that piece 2 does is structural rather than a second pass over the
text, and some shapes fall outside what the parse can hand it. Each has its own
entry:

* a **backquote body inside `" … "`** is text, not parts —
  TD-OILS-A-BACKQUOTE-BODY-INSIDE-DOUBLE-QUOTES-IS-NOT-GOBBLED (which also notes
  that a `<( … )` / `>( … )` body is not descended into).

(Two were fixed since. A **redirection target** was one — the scan never reached
it because the target was not brace-expanded at all:
TD-OILS-A-REDIRECTION-TARGET-IS-NOT-BRACE-EXPANDED. A `' … '` in a **subscript
or a substring bound** was the other — it was read as a quote, so the
substitution it hides was not in the parse for the scan to find; the run now
carries its arithmetic reading beside its text, and the scan follows that reading
when it is inside `" … "`:
TD-OILS-A-SINGLE-QUOTED-RUN-IN-A-BARE-SUB-WORD-OF-A-BRACE-IS-A-QUOTE.)

The `$' … '` splice named above was a further one: bash stores the *translation*
and the gobbler meets a bare `$( … )` in it, echoing `` `fi)}]"' `` with no `'`,
where osh raised only a `bare_splice` flag and parsed the spliced text like any
other — so `"${z:-$'$(fi)'}"` was a script syntax error.

**Fixed 2026-08-14.** The flag became a *list of ranges*,
`Lexer::bare_splices`: where in the body the third row (parse.y:3887) wrote
without reading. They ride `Seg::ParamBraced`'s fourth field, are shifted by
`shift_ranges` at every site that splices one buffer into a longer one (the same
rule `CmdSubSpan::range` follows), are clipped into the operand's own
coordinates by `parser::operand_splices`, and reach
`Lexer::read_word_verbatim` through `lex_operand_in_dquote` as *unread windows*
— inside one, `here_text` is set exactly as a `' … '` run sets it, so a
`$( … )` there is `CmdSubBody::Unread` and is met by the scan that reads the
stored word rather than by a parser.
Corpus: `tests/corpus/an-ansi-c-translation-spliced-bare-into-a-brace-operand-was-never-read.sh`.

Rows 5 and 6 of that case were held back at first, and they are *not* the
splice's: written past a `#` or `//` the translation is re-quoted
(parse.y:3866), and reaching bash's answer for `"${z#$'$(fi)'}"` needed the run
the re-quoting made to be gobbled — TD-OILS-A-QUOTE-RUN-IN-A-PATTERN-OPERAND-IS-NOT-GOBBLED,
which the same probe found, which is not about `$' … '` at all
(`"${z#'$(fi)'}"` diverges on its own), and which is now fixed. Both rows are
back in the case.

**Residual — the splice that never closes.** `echo "15[${z:-$'$(fi'}]"` splices
`$(fi` with no `)`, so the scan runs off the end of the word. bash's reader is
still looking for the `)` and then for the `"`, and says
`command substitution: line N: unexpected EOF while looking for matching `"'`;
osh's `expansion_body_len` answers `Unclosed` first and the body is deferred as
text, which the runtime reports as
``bad substitution: no closing `}' in "15[${z:-$(fi}]"``. `$?` is 1 either way
and the shell carries on either way, so only the wording differs. Reaching bash's
is the unclosed-`$( … )`-inside-an-unread-word shape, not the splice's — see
TD-OILS-A-COMSUB-THAT-NEVER-CLOSES-HIDES-THE-ERROR-INSIDE-IT.
