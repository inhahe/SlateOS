### TD-OILS-HEREDOC-IN-CMDSUB-STOPS-AT-PAREN. a here-document inside `$( … )` did not swallow the closing paren — ✅ RESOLVED 2026-07-30

**Where:** `userspace/oils/src/lexer.rs` — the scan that finds a `$( … )` / `<( … )`
body's extent, which located the closing `)` without knowing that a pending
here-document's body may run straight through it.

**Symptom.** bash reads a substitution's body in the *enclosing* input stream, so a
here-document declared inside one takes its body from the lines that follow — and
those lines can run past the `)`, which is then body text:

```
$ bash -c 'x=$(cat <<EOF
body
); echo "x=[$x]"'
bash: line 3: warning: here-document at line 1 delimited by end-of-file (wanted `EOF')
bash: -c: line 4: unexpected EOF while looking for matching `)'
$ osh -c '…'   # before
osh: line 3: warning: here-document at line 3 delimited by end-of-file (wanted `EOF')
x=[body]
```

Measuring the whole shape (twelve `$( … )` contexts plus five procsub/nesting ones)
turned up four more divergences the entry had not named, all worse than this one:

| input | bash | osh (before) |
|---|---|---|
| `$(cat <<EOF` / `body` / `); echo inner` / `EOF` / `echo after` | `unexpected EOF … matching )` | ran `inner`, then `EOF: command not found`, then `after` |
| `$(cat <<EOF` / `) one` / `EOF` / `echo …` | same | ran `one` **and** `EOF` as commands |
| `cat <(cat <<EOF` / `body` / `); echo after` | warning + `unexpected EOF` | silently ran the substitution and `after`, no warning |
| `$(echo \))` | `)` | `syntax error near unexpected token )` |

**Fix.** `Lexer::read_subst_body` — a here-document-aware mode of
`read_balanced` used by the `$( … )`, `<( … )`/`>( … )` and `${…$(…)…}` call sites.
It consumes each declared here-document's body from the enclosing input at the next
newline and appends the lines **verbatim** to the substitution's raw text, so they
sit exactly where an inline body would have and the later re-lex of that text finds
them, while the scan resumes looking for the `)` from *after* them.

Finding a `<<` at all is what forces the rest: the mode has to recognise the places
one can sit without being an operator — a comment, a `<<<` here-string (consumed
whole, so its second `<` is never read as the first of a `<<`), a shift inside
`$(( … ))` / `(( … ))` (tracked by the paren depth the span began at), a `${ … }`
body, a backtick body (bash lexes those on their own, so the here-document inside
one is *its* business and the closing backtick is found lexically), and a backslash
escape. Handling the last two also fixed `$(echo \))` and
`` $(echo `echo )`) `` as a by-product.

Two supporting changes:

* `Lexer::next_tok_index` — the index of the token the current `run_into` iteration
  is about to push. The word readers are not given `out`, so a reader-level record
  raised deep inside a word (the here-document EOF warning) had no other way to name
  the token it belongs to.
* `IncrementalParser::next_unit` — a parked lexer error about to be *reported* now
  lifts the release frontier to `usize::MAX`. Reaching it means the reader consumed
  the whole input, bodies included; and when the failing scan is the one that
  swallowed the body, the cut back to the last complete line has dropped the very
  token the record names (the stream can be empty, making `orig.len()` 0 and the
  `tok_index < next_orig` gate unsatisfiable). Without this the warning was recorded
  and never printed.

**Verified:** `tests/corpus/heredoc-in-cmdsub.sh` (byte-for-byte, including the
warning-then-syntax-error order and the earlier lines running first) and
`lexer::tests::substitution_body_reaches_past_a_here_document`. Three divergences in
the same scanner remained, each logged on its own below and all three since fixed:
TD-OILS-CMDSUB-HEREDOC-PAST-CLOSE, TD-OILS-CMDSUB-CASE-PATTERN-PAREN,
TD-OILS-CMDSUB-ARITH-VS-SUBSHELL. Found while fixing
BUG-OILS-HEREDOC-EOF-WARNING.
