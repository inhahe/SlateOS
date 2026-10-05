### TD-OILS-CMDSUB-HEREDOC-PAST-CLOSE. a here-document whose `)` is on its own line is not fed from past that line — 2026-07-30 — ✅ RESOLVED 2026-07-30

**Symptom.** When the `<<` and the `)` are on the *same* line, the body lies after
the `)` — outside the substitution's own text entirely. bash notices, warns in a
shape it uses nowhere else, and then *still* delivers the body:

```
$ bash -c 'x=$(cat <<EOF); echo "x=[$x]"
body
EOF'
bash: line 1: warning: command substitution: 1 unterminated here-document
x=[body]
$ osh -c '…'
osh: line 1: warning: here-document at line 1 delimited by end-of-file (wanted `EOF')
x=[]
osh: line 2: body: command not found
osh: line 3: EOF: command not found
```

**Cause.** The extent scan reached `)` with the here-document still pending and
simply dropped it, so the body lines stayed in the enclosing input and were run as
commands.

**Impact (before the fix).** Adversarial: it needs `<<` and the closing `)` on one
line with the body after. Deliberately kept out of
`tests/corpus/heredoc-in-cmdsub.sh` at the time.

**Fixed.** The original write-up above guessed that the *enclosing line's* gather
picks the body up, which would have needed a deferred splice into an already-built
`Seg::CmdSub`. Measurement disproved it: bash gathers **at the `)`**. (Proof:
`x=$(cat <<A) $(cat <<C)` over four body lines blames the second warning on line 3,
so the reader had already consumed A's body before the second `$(` was scanned.)
That makes the fix local:

- `Lexer::gather_ahead` — at the `)`, warn, jump the cursor to the start of the
  next line, run the ordinary `consume_subst_heredoc_bodies` into the
  substitution's raw text (prefixed with a `\n`, since the scan stopped at `)`
  and never reached one to terminate the command with), then put the cursor back
  and record where the reader got to in `Lexer::hd_ahead`.
- `Lexer::sync_ahead` redeems that record when the cursor reaches the end of its
  line, so a body is read exactly once and a second substitution on the same line
  resumes from where the first stopped rather than re-reading.
- `Lexer::fetched_line` and `Lexer::stamp_lines` consult `hd_ahead`, because
  bash's `line_number` is the last line the *reader* fetched: `$LINENO` and any
  diagnostic from the rest of that line name the body's last line.
- `read_balanced_inner` counts here-documents declared *directly* in this body
  (`own`) apart from a nested substitution's. Both have to be fetched — the text
  being copied runs over those lines — but only the direct ones are this
  reader's to warn about; the nested ones are warned about when that body is
  re-lexed as an input of its own.
- `Lexer::heredoc_eof` became `Lexer::warnings: Vec<ReaderWarning>`, a single
  ordered channel, because the relative order of the two warning kinds is
  observable (`x=$(cat <<EOF); …` with no body prints `command substitution:`
  first, then `here-document … delimited by end-of-file`).
- `consume_subst_heredoc_bodies` appends the missing delimiter line when a body
  runs to EOF, so the captured raw text is self-contained and the re-lex of it
  does not warn a second time.

**Verified:** `tests/corpus/heredoc-past-close.sh` (byte-for-byte) and
`lexer::tests::a_substitution_gathers_a_here_document_from_past_its_close`, plus 23
of 24 hand-measured shapes against bash 5.2.37. The 24th is logged as
TD-OILS-CMDSUB-HEREDOC-NESTED-GATHER-ORDER below.
