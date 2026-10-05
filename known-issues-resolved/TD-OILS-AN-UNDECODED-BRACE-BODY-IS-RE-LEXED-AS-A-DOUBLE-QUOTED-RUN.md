### TD-OILS-AN-UNDECODED-BRACE-BODY-IS-RE-LEXED-AS-A-DOUBLE-QUOTED-RUN. A `<(`/`>(` in it is never read, though the brace scan names it — 2026-08-14 — ✅ FIXED 2026-08-14

**Where:** `userspace/oils/src/interp.rs` — `Shell::extent_read_of_rest` and
`Shell::unclosed_brace_reads`, both of which lex their text with
`crate::parser::dquote_word_from_source` → `crate::lexer::lex_dquote_body`.

**What is wrong.** `extract_dollar_brace_string` names `$(`, `<(` and `>(`
together and hands each to the same `extract_command_subst` (subst.c:1881-1950),
**whatever the quoting** — that is why `x='A${z#<(fi)}B'` reports the parse
twice. A double-quoted *run*, by contrast, has no process substitution in it at
all: at string level bash and osh agree that `v='A<(echo hi⏎q'` is literal
text. So `lex_dquote_body` is the right lexer for a string-level remainder and
the wrong one for text the **brace scan** is walking.

Measured (`build/pgY.sh` d6), `A${z:-P1<(echo hi⏎S1}B` under `${…@P}`:

| | bash 5.2.37 | osh |
|---|---|---|
| reports | `` unexpected EOF while looking for matching `)' `` **then** `…: bad substitution` | the `bad substitution` only |
| value | undecoded word | same |

The `$(` spelling of the same row (`build/pgW.sh` row 5) is byte-exact, so this
is precisely the two openers `lex_dquote_body` cannot see. The dollar spelling
of d7 — where the read stops early and the brace closes — is also exact,
because that path re-lexes through `parse_braced_param_in` in
`Quoting::Unread`, which *does* read them.

**It is not only the two openers — the whole quote model is wrong** (measured
2026-08-14, `build/pq1.sh` and `build/pq2.sh`). `extract_dollar_brace_string`
**skips** a quoted run rather than walking it, and the two quotes skip
differently:

| word (inside `v='…'`, via `"${v@P}"`) | bash 5.2.37 | what it shows |
|---|---|---|
| `A${z:-P1<(echo hi⏎S1}B` | reports EOF, `bad substitution` | the bare `<(` row **is** read |
| `A${z:-P1"<(echo hi⏎S1"}B` | silent, `[AZZB]` | a `<(` inside `" … "` is **not** |
| `A${z:-P1"$(echo hi⏎S1"}B` | reports EOF, `bad substitution` | a `$(` inside `" … "` **is** |
| `A${z:-P1'$(echo hi⏎S1'}B` | silent, `[AZZB]` | a `$(` inside `' … '` is **not** |
| `A${z:-P1'<(echo hi⏎S1'}B` | silent, `[AZZB]` | …nor a `<(` |
| `A${z:-P1"<(echo hi⏎S1}B` | `bad substitution`, **no** read report | a lone `"` swallows to end of string |
| `A${z:-P1'<(echo hi⏎S1}B` | `bad substitution`, **no** read report | …and so does a lone `'` |
| `A${z:-"x"<(echo hi⏎S1}B` | reports EOF, `bad substitution` | a *closed* run does not suppress what follows |
| `A${z:-P1\<(echo hi⏎S1}B` | silent, `[AZZB]` | a backslash escapes the opener |

So the brace scan delegates a `" … "` run to a double-quote skipper that has
the `$(` row and **not** the `<(`/`>(` row — bash's ordinary rule that there is
no process substitution inside double quotes — and skips a `' … '` run whole,
offering its interior to nothing.

`lex_dquote_body` models neither — it treats both quote characters as ordinary
literals, which is correct for `Q_DOUBLE_QUOTES`, where the string *is* already
the quoted run. Measured (`build/pq1.sh`, `build/pq2.sh`, `build/pq3.sh`), osh
nevertheless agrees with bash on every *quoted* row above, by a different
mechanism in each case: where the run closes, the brace closes too and the word
goes through `parse_braced_param_in`, which does model quotes; where the run does
not close, `lex_dquote_body`'s missing `<(` row happens to suppress the same read
bash's skip suppresses. Two rows were left where the mechanisms did not coincide;
the first of them is now fixed:

| word (inside `v='…'`, via `"${v@P}"`) | bash 5.2.37 | osh |
|---|---|---|
| `A${z:-P1"$(echo hi⏎S1}B` | reports EOF, `bad substitution`, undecoded | ✅ same since 2026-08-14 |
| `A${z:-'p$(echo hi'q$(fi⏎S1}B` | reports `fi`, `[AZZB]` | silent, `[AZZB]` |

Row 1 was the serious one — a **spurious command execution**: osh reported the
EOF, then ran `S1}` and produced `[Ahi]`. A lone `"` opens a run that swallows to
end of string, leaving the brace nothing to close on, so bash condemns the word;
osh instead let the failed read out of `read_opaque_span`'s `"`-run `$(` sub-arm,
where [`Lexer::unclosed_seg`] degraded the whole word into a *string-level*
`$( … )` and then performed it. Fixed 2026-08-14 by giving that sub-arm
(`userspace/oils/src/lexer.rs`, `read_opaque_span`'s `'"'` arm) the same
`Err(e) if self.unread_comsub(&e)` recovery the two `read_dollar_brace_body`
arms already had: re-emit the `$(` into the raw text, take back what the reader
consumed with `Lexer::unread_comsub_stop`, and `continue` the quoted-run loop.
The read is still reported — it happened — and the run then swallows the rest,
so the brace never closes and the word is condemned, exactly as in bash. The bug
was **pre-existing**, not a regression: measured identical on the commit before
the earlier 2026-08-14 brace-scan fix.

Row 2 is a lost diagnostic only; the same row before the brace-scan fix had the
wrong value *and* ran `f`, so it is much improved.

**A second mechanism loses the same report where the brace *does* close**
(measured 2026-08-14, `build/pr1.sh` r3). `A${z:-'i"t'<(fi⏎S1}B` reports `fi`
in bash and expands to `[AZZB]`; osh now gets the value right (it was the
undecoded word until the unmated-`"` fix of the same day) but still says
nothing. That path never goes near `extent_read_of_rest`: the brace closed, so
the reads are replayed off the *parsed operand*, and the operand lexer is
`read_word_verbatim` in [`Verbatim::Dquote`] — which has a perfectly good `<(`
row, but never reaches it, because the `"` inside the `' … '` run opens a
quoted run that swallows `t'<(fi⏎S1` whole.

Both scans are right about their own text and wrong about each other's, which
is the shape of the whole issue: bash runs **two** passes over these bytes with
**different quote rules** — `extract_dollar_brace_string`, where a `'` run is
skipped and a `"` is a quote, and `expand_word_internal`, where a `'` is an
ordinary character and a `"` is a quote. osh derives the reads from the
expansion's lex in one path and from a string-level lex in the other, and
neither is the scan's.

**What the proper fix looks like.** A real lex entry for "text a brace scan is
walking" — not `lex_dquote_body` with a row bolted on, and not the operand lex
either. It needs, at its own level: the `<(`/`>(` openers beside `$(`; a `'`
that consumes to the next `'` or to end of string, offering nothing inside it;
and a `"` that consumes to the next `"` or to end of string, offering only `$(`
(and `` ` ``) inside it. A backslash hides the byte after it. Then
`extent_read_of_rest`, `unclosed_brace_reads` **and `brace_extent_scan`** all
take their reads from that one pass, `lex_dquote_body` keeps its current
string-level callers unchanged — the p1/p2 probe above confirms those answers
are right as they stand — and the operand lex stops being asked a question it
was never answering.

These rows are the acceptance test the table above does not already cover — the
ones that pin *which* quote wins when the two are interleaved (measured
2026-08-14 against bash 5.2.37, `build/pr1.sh`):

| word (inside `v='…'`, via `"${v@P}"`) | bash 5.2.37 | osh today |
|---|---|---|
| `A${z:-"it's"$(fi⏎S1}B` | reports `fi`, `[AZZB]` | same |
| `A${z:-"it's"<(fi⏎S1}B` | reports `fi`, `[AZZB]` | same |
| `A${z:-'i"t'<(fi⏎S1}B` | reports `fi`, `[AZZB]` | ✅ same since 2026-08-14 |
| `A${z:-P1\'<(echo hi⏎S1}B` | reports EOF, `bad substitution` | ✅ same since 2026-08-14 |
| `A${z:->(echo hi⏎S1}B` | reports EOF, `bad substitution` | ✅ same since 2026-08-14 |
| `A${z:-${y:-<(fi⏎S1}B` | reports `fi`, `bad substitution` | same |

So a `'` inside a closed `" … "` run opens nothing (rows 1-2) and a `"` inside a
closed `' … '` run opens nothing (row 3) — each quote is invisible inside the
other's run — and a backslash spends itself on the quote it precedes, leaving
the `<(` after it live (row 4).

An attempt that added only the `<(`/`>(` row to `lex_dquote_body` was written
and reverted on 2026-08-14, before being compiled, because these measurements
showed it would have regressed the three suppressed rows above (they are silent
in bash today and in osh today, and would have started reporting).

**Fixed 2026-08-14**, along the lines above. Three pieces:

- `Lexer::brace_scan` (`userspace/oils/src/lexer.rs`) — a flag saying "this
  scan stands in for `extract_dollar_brace_string`, not for the expansion after
  it". With it set, `read_double_quote_until` grows the scan's other two
  openers: a `<(`/`>(` becomes a `SubBody::Unread` segment carrying its own
  `SubDelim`, which the expansion prints straight back
  (`SubDelim::is_performed` is false for both), so the word's **value** is
  untouched and only the extent walk gains a construct to read. The new entry
  `lexer::lex_brace_scan_body` → `parser::brace_scan_word_from_source` is what
  `Shell::extent_read_of_rest` now lexes its remainder with, which is the
  unclosed-brace half (rows 4-5 of the interleaving table above).
- The closed-brace half (row 3) is the same flag turned on from
  `read_word_verbatim`'s `"` arm, and **only** when that run opened inside a
  `' … '` one — `in_run && self.here_text`. That is exactly the case where the
  scan never saw a quote at all, because it stepped over the single quotes
  whole. Outside a run the `"` is the scan's own, and there `skip_double_quoted`
  reads the `$(` spelling alone, which is what the reader already did.
- The quote state itself moved out of the lexer and into the walk, as
  `ScanQuote` (`interp.rs`): two independent flags, because
  `skip_single_quoted` hunts for a `'` and `skip_double_quoted` for a `"` and
  neither knows the other character — so each quote is an ordinary byte inside
  the other's run. `Shell::brace_scanned_subs_slice` tracks both over the
  literal runs (a `\` still hides the byte after it) and suppresses the two
  process-substitution spellings inside a `" … "` while letting `$(` through;
  `brace_scanned_subs_in` no longer resets the state on entering a
  `WordPart::DoubleQuoted` whose `"` the scan never saw.

Corpus case:
`userspace/oils/tests/corpus/the-brace-scan-reads-a-process-substitution-and-the-expansion-after-it-does-not.sh`
— 14 shapes plus a here-document body, byte-identical to bash 5.2.37 including
stderr.

**Impact while it stood.** Diagnostics only — the values already agreed. A
`<(`/`>(` at brace level lost its read report. The worst shape — a lone `"`
before a `$( … )` making osh run a command bash does not, and yield the wrong
value — was fixed earlier the same day (see row 1 of the two-row table above).
Reachable only through `@P`/`PS4`/here-doc text holding a malformed `${ … }`.

**Not fixed by this, and tracked separately:** row 2 of the two-row table,
`A${z:-'p$(echo hi'q$(fi⏎S1}B`. That one is not about the openers but about
where a construct *ends*; see
`TD-OILS-A-SQUOTE-RUN-DOES-NOT-CUT-A-SUBSTITUTION-SHORT-FOR-THE-BRACE-SCAN`.
