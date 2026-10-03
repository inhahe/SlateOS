### TD-OILS-A-FAILED-EXTENT-READ-RUNS-TO-THE-END-OF-THE-STRING. `v='A$(fi⏎echo x⏎)B'; echo "${v@P}"` quotes and consumes to the end where bash stops at the error's own line — 2026-08-09 — ✅ FIXED 2026-08-09

**Where:** `userspace/oils/src/interp.rs`, `Shell::comsub_reparse_read` /
`Shell::failed_extent_body` / `Shell::arith_dolparen`, and
`crate::parser::comsub_reprint_error`'s `line_off`. Not specific to arithmetic
and **not specific to here-documents** — it shows on any body whose failure is
not on its last line, at the bare string level too.

**Reproduce:**

```text
a='A$(fi
echo x
)B';  printf '1 [%s]\n' "${a@P}"        # printf on line 3

  bash: command substitution: line 4: syntax error near unexpected token `fi'
        command substitution: line 4: `fi'
        line 3: f: command not found
        1 [A
        echo x
        )B]
  osh:  command substitution: line 6: syntax error near unexpected token `fi'
        command substitution: line 6: `)B'
        command substitution: line 3: syntax error near unexpected token `fi'
        command substitution: line 3: `fi'
        1 [A]
```

Three things are wrong at once, and all three are the same wrong number: the
reported **line**, the **echoed** line, and where the read **stopped** — which
decides both the text the child runs (`f`, not `fi⏎echo x⏎)`) and how much of
the string is consumed (bash expands `⏎echo x⏎)B` afterwards, osh drops it).

**Why (measured against bash 5.2.37, then read back in the source).** bash's
string reader is line-at-a-time: `shell_getc` fills `shell_input_line` with one
line and `line_number++` counts it (parse.y:2361). A syntax error is reported at
whatever `line_number` then holds and echoes whatever `shell_input_line` then
holds, so **both name the line the error was found on** — and `parse_string`
leaves its pointer just past that line's newline, which is what
`xparse_dolparen` reads back:

```c
if (nc < 0) { … }                          /* parse.y:4330 */
if (ep[-1] != ')')
  { while (ep > ostring && ep[-1] == '\n') ep--; }
nc = ep - ostring;
*indp = ep - base - 1;
ret = (nc == 0) ? "" : substring (ostring, 0, nc - 1);   /* parse.y:4348-4376 */
```

So one position `ep` — the end of the error's line, backed up over trailing
newlines — gives the child's text (`ostring[0 .. ep-1]`) *and* the resume point
(`sindex = ep`). Measured across six shapes (error on line 1, on a later line,
with a here-document on the error line, with a here-document above it, on one
line, after a leading newline) the rule holds without a here-document special
case; a here-doc only matters because gathering its body advances the reader, so
the error's line number counts the body's lines too. The remainder is expanded,
not copied: `a='A$(fi⏎$y⏎)B'` with `y=Y` gives `A⏎Y⏎)B`.

osh instead assumes the reader ran to the end of the text in all three places:
`comsub_reprint_error`'s `line_off = newlines(src) + 1`, `failed_extent_body`'s
"last non-newline, less one", and `extent_consumed`'s "the rest of the string is
gone". Single-line bodies are unaffected, which is why the whole corpus passes.

**Fixed, 2026-08-09.** The one position `ep` is now computed once and both halves
taken from it.

* `crate::parser::ComsubReprintError` gained **`stop_line`** beside `line_off`.
  The two are usually the same number and differ exactly where the *reported*
  line is not the reader's: a `parse_matched_pair` complaint is blamed on the
  line the construct opened, and an `unexpected end of file` on the line the
  `)` was wanted from, while in both the reader really did run the text out —
  those answer `u32::MAX`. `comsub_reprint_error`'s `Ok` branch stopped
  fabricating a line: it now parses through the new `parse_paren_body_mapped`
  with `LineMap::default()`, so `err.line` comes back numbered from 1 *within
  the read's own text*, which is both the line bash reports and the line its
  reader stopped on. That made `close_line` dead on the whole reprint path, and
  it was removed from `comsub_reprint_error`, `Shell::comsub_reparse_error` and
  `Shell::comsub_reparse_read`.
* `Shell::failed_extent_body` became **`Shell::failed_extent_split`**, which
  builds the composite `src + ")" + tail`, walks `stop_line` newlines into it
  (`Shell::reader_stop`), backs up over trailing newlines exactly as parse.y
  does, and returns the pair `(ostring[0 .. ep-1], composite[ep ..])`.
  `ExtentRead::Abandoned` carries both halves.
* `Shell::run_abandoned_extent` runs the first half and appends the second,
  expanded — `crate::parser::dquote_word_from_source` +
  `Shell::expand_double_quoted`, with `extent_consumed` saved across it so a
  leftover holding its own failing `$(` composes rather than truncates. The
  remainder being empty is now what `extent_consumed` means, and it is the only
  case in which a `${ … }` scan is left with no `}` to find.
* `Shell::arith_dolparen` returns `(Str, Option<usize>)` — the child's output
  and, for a read that gave up, the byte of `rest` the scan resumes on.
  `expand_arith_params_inner` moves its cursor there instead of stopping, so the
  leftover is re-scanned *as arithmetic*: `A$[1+$(fi⏎echo x⏎)]B` evaluates
  `1+⏎echo x⏎)` and complains of the error token `x⏎)`, as bash does. The stop
  line comes from the parse error's own line unmapped, or `u32::MAX` when the
  diagnostic has no source echo — the same rule as above, and the same test
  `Shell::format_parse_error` already uses to decide whether to echo one.
* `Shell::brace_extent_scan` gained an arm for a read that stopped part way: the
  brace scan carries on from `ep` and finds the `}`, so there is no `bad
  substitution`. Measured, `y=Y; A${y:-p$(fi⏎q)r}B` reports once and still gives
  `AYB`.

Corpus `a-failed-extent-read-stops-on-the-line-the-reader-reached.sh` (10
probes: the reader's line, the child's byte, the leftover expanded, a recursive
leftover, a here-document body, a body that runs the string out, a stray `)`
coming back, the `$[` expression's leftover, and the brace scan's `}`).
`a-failed-extent-parse-consumes-the-rest-of-the-string.sh` had its header
corrected — it documents the one-line shape, not the rule.

Two follow-ons the measurement turned up have entries of their own:
TD-OILS-AN-ARITHMETIC-EXTENT-CARRIES-ON-COUNTING-AFTER-A-FAILED-READ (fixed
since) and
TD-OILS-A-BRACE-OPERAND-IS-SCANNED-AGAIN-BEFORE-IT-IS-EXPANDED (still open).
