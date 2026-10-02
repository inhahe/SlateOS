### TD-OILS-A-BACKQUOTE-BODY-INSIDE-DOUBLE-QUOTES-IS-NOT-GOBBLED. A backquote body inside `" … "` is run where bash only scanned it — 2026-08-10 — ✅ FIXED 2026-08-14

**Where:** `userspace/oils/src/interp.rs`, `Shell::gobbled_subs` — the walk that
lists the substitutions `brace_gobbler` reads. It is structural, over
`WordPart`s, and a backquote body is kept by osh as text
(`CmdSubBody::Backtick`), not as parts, so the walk cannot descend into one.

**Reproduce.** With `z` unset, a word holding a backquote whose body is
`${z:-'$(fi)'}`, inside double quotes. bash reports `command substitution: line
N: syntax error near unexpected token `fi'` and then the word's remainder, and
prints nothing; osh instead runs the backquote, whose body expands to the text
`$(fi)`, and reports `$(fi): command not found`.

**What bash does.** The gobbler's `quoted` is set by a backquote only from the
unquoted state:

```c
  if (quoted)
    {
      if (c == quoted) quoted = 0;
      if (quoted == '"' && c == '$' && text[i+1] == '(') goto comsub;
      ADVANCE_CHAR (text, tlen, i); continue;
    }
  if (c == '"' || c == '\'' || c == '`') { quoted = c; i++; continue; }
                                                   /* braces.c:660-675 */
```

Inside `" … "` the `quoted` branch is taken first, so a backquote is only a
character and the scan reads straight on into the body — where the `${` is
treated like `\{`, the `'` is not a quote, and the `$( … )` is handed to
`extract_command_subst`. At the *top* level the backquote does set `quoted`, and
then no `$(` row is reached, so the body is skipped; leaving backquotes out of
the walk is right there and wrong only inside double quotes.

**The fix — done 2026-08-14.** `Shell::gobbled_subs` now lexes a backquote body
as the double-quoted text the scan takes it for — `parser::dquote_word_from_
source`, then `unparse::gobbler_word` to re-scope it — and walks the result,
but only when `dquoted`. Descending stays off at the top level, where
`gobbler_reads`'s `false` for every `CmdSubBody::Backtick` was already right.

Two things came with it:

* **The remainder a diagnostic quotes.** The lexed body's tails stop at its
  end; the gobbler's do not, because it was handed the whole word. So
  `CmdSubBody::Backtick` gained a `tail` field, filled by `gobbler_word` alone
  (no parser wants it — a backquote body is `string_extract`'s byte hunt for
  the closer), and each substitution the body contributes gets
  `body tail` + `` ` `` + that. Measured: ``echo "p`echo "$(fi)"`q{,}"`` quotes
  ``fi)"`q{,}"``.
* **The `{` gate.** `gobble_scan` had relied on "a `${` carries its own `{`" to
  subsume `brace_expand_word_list`'s `mbschr (…, LBRACE)` (subst.c:9905). A
  backquote body carries no `{`, so the gate is now tested for real against the
  word's source — ``echo "p`echo $(fi)`q"`` reaches no scanner where the same
  word with a `{,}` on the end does.

Corpus: `a-backquote-body-inside-double-quotes-is-read-by-the-brace-scanner.sh`.

Note the same walk reaches nothing inside a `<( … )` / `>( … )` body either,
because `crate::unparse::nested_parts` returns no scope for a `ProcSub`.
