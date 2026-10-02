### TD-OILS-A-QUOTE-RUN-IN-A-PATTERN-OPERAND-IS-NOT-GOBBLED. `"${z#'$(fi)'}"` runs where bash drops the command — 2026-08-14 — ✅ FIXED 2026-08-14

**Where:** `userspace/oils/src/interp.rs`, `Shell::has_gobbled_sub` (~27065) and
`Shell::gobbled_subs`. Both descend into a `WordPart::SingleQuoted` only through
its `parts` — the run's *second* reading, which is filled in for a subscript and
a substring bound (`word_subscript_from_source_at`) and nowhere else. A pattern
and a replacement carry `parts: None`, so whatever the run hides is invisible to
the scan.

**What is wrong.** Inside `" … "` the gobbler's `quoted` is `"`, so its `'` row
(braces.c:670) is never reached and the run is *not* a quote to it: the `$(` row
of the `quoted == '"'` branch fires on what is inside. bash's parser, meanwhile,
did read the run as a quote — under `DOLBRACE_QUOTE` a `'` quotes (parse.y:3836,
3840) — which is why the diagnostic's remainder still shows the `'`. So the two
readers disagree by design, and only one of them is modelled here.

This is the same disagreement as
TD-OILS-A-SINGLE-QUOTED-RUN-IN-A-BARE-SUB-WORD-OF-A-BRACE-IS-A-QUOTE (fixed) and
TD-OILS-A-QUOTE-INSIDE-A-GOBBLED-BACKQUOTE-BODY-DOES-NOT-END-THE-RUN (fixed), in
a third place.

**Reproduce.** With `z` unset:

```sh
echo "1[${z#'$(fi)'}]";     echo "1 rc=$?"
echo "2[${z//x/'$(fi)'}]";  echo "2 rc=$?"
echo "3[${z%'$(fi)'}]";     echo "3 rc=$?"
echo "4[${z:-'$(fi)'}]";    echo "4 rc=$?"   # already agrees
```

| | bash 5.2.37 | osh |
|---|---|---|
| 1–3 stdout | — | `1[]` / `2[]` / `3[]` |
| 1–3 stderr | ``command substitution: … `fi)'}]"' `` | — |
| 1–3 `$?` | 1 | 0 |
| 4 | agrees (`$?` 1, same remainder) | agrees |

Row 4 agrees because a `:-`-style operand keeps its `'` as a *character* and the
substitution stays a part of the word, which the walk already finds.

**Fixed 2026-08-14.** Not new machinery — the same second reading the subscript
case already carries, filled in one more place. `unparse::fill_quoted_runs`
runs first thing in `gobbler_word`, the copy `gobble_scan` makes for the scan,
and gives every `SingleQuoted` inside `" … "` a `parts: Some(…)` lexed from its
text with `crate::parser::dquote_word_from_source` — which is what the run *is*
to the gobbler (more double-quoted text) and which builds none but
`CmdSubBody::Unread` bodies, so a body that will not parse stays a runtime
diagnostic and not a parse error.

Everything downstream then works unchanged: `unparse::part_src` already prints
such a run from its parts rather than its text, so `attach_tails_by`'s sentinel
shows through the quotes and the tails come out measured against the *whole
word*, `walk_parts_in` already descends through a filled run, and
`gobbled_subs`'s existing `SingleQuoted` arm collects what is inside. The one
other change is in `has_gobbled_sub`, which runs *before* the fill and so has to
answer from the text: inside `" … "` the only gobbler row that fires between the
quotes is `$(`, so a run whose text holds `$(` is answered yes — wide on
purpose, exactly as a backquote inside `" … "` already is, since a scan that
finds nothing reports nothing.

Two runs are deliberately left alone, both because filling them would move every
tail after them: a backslash-escaped character (`a\*b`, which is a
`SingleQuoted` with `escaped`), whose source is not a quoted run at all; and one
whose reading does not re-print to the very same bytes, which `fill_quoted_runs`
checks for and declines.

**Carried the same flat-state limitation as the backquote fix, for one commit.**
The run's text was lexed as *one* double-quoted word, so `quoted` stayed `"` for
the whole of it — where bash's loop lets a `"` **inside** the run set
`quoted = 0`, after which a following `'` or `` ` `` opens a skip the `$(` row
cannot fire in. That was
TD-OILS-A-QUOTE-INSIDE-A-GOBBLED-BACKQUOTE-BODY-DOES-NOT-END-THE-RUN reached
through a second construct, and it is fixed there (2026-08-14) for both places
at once: `wordscan::gobbler_readable` runs the flat loop over the verbatim text
and names the stretches the `$(` row can fire in, `unparse::gobbler_reading`
lexes those and keeps the rest as `WordPart::Literal`, and the run still
re-prints to its own bytes.

**Found by** the `$' … '` bare-splice work
(TD-OILS-A-SINGLE-QUOTED-COMMAND-SUBSTITUTION-IN-A-BRACE-OPERAND-IS-PARSED): the
`#`/`//` rows of its corpus case are exactly this, since a translation written
past a `#` is re-quoted (parse.y:3866) into such a run. Those two rows were held
out of
`tests/corpus/an-ansi-c-translation-spliced-bare-into-a-brace-operand-was-never-read.sh`
and are back in it now (rows 5 and 6).
