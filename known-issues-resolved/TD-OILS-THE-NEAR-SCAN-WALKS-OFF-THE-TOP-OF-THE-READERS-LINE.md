### TD-OILS-THE-NEAR-SCAN-WALKS-OFF-THE-TOP-OF-THE-READERS-LINE. `near \`)\'` where bash names a newline — 2026-08-08 — ✅ FIXED 2026-08-08

**Where:** `userspace/oils/src/parser.rs` — `Spans::near`, the port of bash's
`error_token_from_text` (parse.y). Its only caller is `Parser::cond_near_at`, so
only conditional `near` lines are affected.

**What.** bash's scan reads `t = shell_input_line` — **one line**, indexed from
0. osh's port keeps the whole input text and an absolute offset into it, which
agrees with bash everywhere except at the one place the difference shows: when
the reader stopped at *offset 0 of its line*. bash's first loop (`while (i &&
…)`) does not run at all there, `token_end` stays 0, and it returns the single
character `t[0]`. osh has a non-zero absolute offset, so it walks back across the
line boundary and drags in text from the line before.

A reader lands on offset 0 exactly when a `\<newline>` flush against the
offending token was deleted and the fetch brought a new line in. If that line
starts with a non-space, the two models agree by accident — bash's one-character
branch and osh's walk both yield that character. If it starts with whitespace
they diverge:

```text
echo 1⏎[[ a == b )\⏎⏎        bash:  line 3: syntax error near `⏎'
                                    line 3: `'
                             osh:   line 3: syntax error near `)\'
                                    line 3: `'
echo 1⏎[[ a == b )\⏎   ⏎      bash:  line 3: syntax error near ` '
                             osh:   line 3: syntax error near `)\'
echo 1⏎[[ a b\⏎⏎             bash:  line 3: syntax error near `⏎'
                             osh:   line 3: syntax error near `b\'
```

Found while fixing TD-OILS-A-BACKSLASH-CLOSED-STRING-STILL-FINDS-A-TOKEN-TO-REPORT-NEAR,
which removed the *other* half of that report (the emptied-buffer branch). This
half survives because here the fetch did find a line — a blank one.

**Fixed.** `Spans::near` was given bash's floor. Where it used to walk the whole
input text from an absolute offset, it now slices the reader's own line out of it
first — from just past the previous `\n` through the `\n` that ends it — and
makes the offset line-relative. The walk below is untouched, but every step of it
now means what it means in bash: the `i > 0` guards, the `token_end` sentinel and
the `token_end == 0` one-character fallback.

The continuation case needed no special handling, which is the point: it is the
same line rule. `shell_getc` *replaces* `shell_input_line` with the fetched line
rather than splicing it onto the old one, so a reader dragged past a deleted
`\<newline>` is genuinely at index 0 of a fresh line, and clamping to that line
reproduces it. An alias replacement has no newline in it, so the clamp is the
whole of that text and costs nothing there.

**Pinned by** `parser.rs`'s `the_near_scan_cannot_reach_back_past_the_readers_own_line`
(the three divergent rows plus the agree-by-accident and mid-line-stop rows that
keep the floor from moving) and the corpus case
`the-near-scan-cannot-reach-back-past-the-readers-own-line.sh`, which walks the
blank, space, tab, ` x`, `;` and non-blank continuation lines against bash.
