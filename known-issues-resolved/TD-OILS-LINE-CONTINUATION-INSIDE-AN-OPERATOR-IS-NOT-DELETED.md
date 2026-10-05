### TD-OILS-LINE-CONTINUATION-INSIDE-AN-OPERATOR-IS-NOT-DELETED — 2026-08-05 — ✅ FIXED 2026-08-05

**Where:** `userspace/oils/src/lexer.rs` — `\<newline>` is removed while scanning
a *word*, but not in the character stream the operator scanner reads.

**What.** bash deletes `\<newline>` in its **reader** (`shell_getc` in parse.y
throws both characters away and reads on), so by the time anything is tokenized
the pair is simply gone — it can sit anywhere, including between the two
characters of a two-character operator, or between a `$` and its `(`. osh
removes it only inside word scanning, so every other site mis-lexes:

```text
true |\<nl>| echo second   bash: `||`, prints nothing   osh: `true |  | echo second`, prints second
false &\<nl>& echo second  bash: `&&`, prints nothing   osh: prints second
echo a;\<nl>; b            bash: `;;` — syntax error    osh: two `;` — runs `b`
echo x >\<nl>> f           bash: `>>` f                 osh: `>` then `>` — ambiguous redirect
cat <\<nl><EOF             bash: heredoc                osh: ambiguous redirect, body runs as commands
echo $\<nl>(echo hi)       bash: prints hi              osh: syntax error near `('
```

The one-line spellings all agree (`true | | echo second` is a syntax error in
both), and words are already right (`ec\<nl>ho hi` prints `hi`), as are `[\<nl>[`
and `]\<nl>]`. Worse than the diagnostics: three of these rows are **silently
wrong output**, not an error — `|\<nl>|` runs the right-hand side of what the
author wrote as `||` unconditionally.

**Impact.** Any script that wraps a long line at an operator by splitting the
operator itself. Rare in hand-written code, but the failure mode is silent.

**Fixed 2026-08-05.** The deletion is now a property of the *cursor*, not of the
word scanner, which is as close to bash's reader as osh can get without giving
up byte offsets into the original text (`Spans`, `Tokenized::ends` and the
command history all index it). `Lexer` gained five primitives in `lexer.rs`:
`cont_len_at(i)` (2 for `\<newline>`, 3 for the `\<CR><LF>` a CRLF file writes,
`None` otherwise), `cont_skip(i)` for the scans that look ahead by index before
deciding to move at all, `eat_conts()` which deletes every continuation standing
at the cursor — recording each in `conts` and calling `sync_ahead` so a
here-document read ahead on that newline is redeemed exactly as `bump_ch`
redeems it — `adv()` (one character, then `eat_conts`), the step to take *inside*
a multi-character operator, and `seek(to)` for the scans that settle where to
land by index, so the continuations passed on the way are still recorded.

Every operator arm in `run_into` now steps with `adv()`, so `|\<nl>|`, `&\<nl>&`,
`;\<nl>;`, `;\<nl>&`, `>\<nl>>`, `<\<nl><`, `<\<nl>>`, `>\<nl>&`, `>\<nl>|`,
`|\<nl>&` and `(\<nl>(` all form the operator bash forms. The lookahead guards
use `cont_skip`: the `<\<nl>(` process substitution (in both `run_into` and
`read_word`), `$\<nl>((`, and the digits of an IO number, which are gathered
character by character rather than sliced so `2\<nl>>&1` is still the number 2.
`varfd_prefix` builds its name the same way, and `read_dollar`, `read_arith` and
`read_dollar_brace` treat a `\<newline>` as absent text rather than an escape, so
`$\<nl>v`, `${v\<nl>x}`, `${#\<nl>w}` and `$((1+1)\<nl>)` all read as the
one-line spelling does.

Two invariants needed care. The eat-conts loop at the top of `run_into` runs
*after* `start_pos` is captured, so the newline falls inside the iteration's
span where `stamp_lines` already counts it — putting it before `start_pos` would
have regressed `$LINENO` on a line that opens with a continuation. And the `$((`
backtrack now restores `conts` as well as `pos`: the same text is about to be
re-read as a substitution body, which keeps its continuations.

The reader's three exemptions are unchanged and still correct, because they are
the sites that never reach these primitives: `'…'`, a here-document with a
quoted delimiter, and a comment (which therefore still ends at its newline).

**Pinned by** `tests/corpus/a-line-continuation-is-deleted-before-anything-reads-it.sh`.
