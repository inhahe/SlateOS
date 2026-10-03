### TD-OILS-HISTEXPAND-BACKSLASH-LINE. A line ending in `\` is treated as complete, so the continuation line after it never reaches history expansion — ✅ RESOLVED 2026-07-29

**Where:** `userspace/oils/src/interp.rs` — `needs_more_lines()`, and through it
the loop in `expand_history_lines()`.

**What.** With `set -o history; set -H` and `echo one` recorded:

```
echo x \
!!
```

bash prints `x echo one` (echoing the rewritten `echo one` first); osh prints
`x !!` and echoes nothing.

The reader loop in `expand_history_lines` offers one physical line at a time and
stops as soon as `needs_more_lines(&accum)` says the lines so far form a runnable
command. `needs_more_lines` answers by parsing, and the lexer *deletes* a
`\<newline>` as a line continuation — so `"echo x \\\n"` parses as the
complete command `echo x`, the loop returns, and the `!!` on the next line is never
offered. The parser then joins the two lines itself and runs the `!!` literally.

Note this is *not* the quote-state bug (TD-OILS-HISTEXPAND-LINE-QUOTE-STATE,
resolved above): `lexer::open_quote` correctly reports `None` here, and the
continuation line simply never gets expanded at all. It is the reader's
*frontier* that is wrong, not the context it expands in.

**Fix (2026-07-29).** `lexer.rs` gained
`ends_in_continuation(src, opts) -> bool`, and `needs_more_lines` returns `true`
when it says so, before parsing at all.

The test is deliberately not textual, because a trailing `\` is not always a
continuation: inside `'…'` or a quoted-delimiter here-document body the lexer
keeps it, and with no newline after it there is nothing to continue onto. So the
predicate asks the lexer what it *actually deleted* — `tokenize_deferred` already
returns `conts`, the offset of every backslash whose `\<newline>` it removed —
rather than re-deriving the rule and risking the two drifting apart.

One measured detail: `\<CR><LF>` is **not** a continuation. The `\` escapes the CR
and the newline then ends the line, so a CRLF script's `echo x \` prints `x \r`
and joins nothing — bash on this host does the same. The predicate therefore
keys only on a `\` immediately before the final `\n`.

**Tests.** The `echo x \` / `!!` section of
`tests/corpus/histexpand-continuation.sh`, together with the `echo 'x \` / `!!'`
negative case, plus the unit test
`lexer::tests::ends_in_continuation_asks_which_backslashes_were_deleted` (which
covers the `'…'`, quoted-here-doc, escaped-backslash, no-newline and
already-joined cases).
