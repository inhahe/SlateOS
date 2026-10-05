### TD-OILS-A-HERE-DOCUMENT-OPERATOR-WITH-NO-DELIMITER-WORD-WAS-NOT-A-GRAMMAR-ERROR. `cat << ; echo hi` ran, and `cat <<#c` took the comment as a delimiter — 2026-08-06 — ✅ FIXED 2026-08-06

**Where:** `userspace/oils/src/lexer.rs` — `Lexer::lex_heredoc_op` and
`Lexer::read_heredoc_delim`.

**What.** `<<` is a redirection operator and its target is an ordinary WORD —
bash's grammar says so literally (`redirection: … LESS_LESS WORD`, parse.y). A
`<<` with no word after it is therefore a *grammar error*, named at whatever
token turned up in the WORD's place, exactly as a `<` or `>` with no target is.
osh instead built a here-document with an empty delimiter, which no body line
can equal, so the gather ran to the end of the input and warned:

```text
cat << ; echo hi       bash: syntax error near unexpected token `;'
                       osh:  warning: here-document … (wanted `'), then ran `hi`
cat << >f              bash: syntax error near unexpected token `>'
cat <<                 bash: syntax error near unexpected token `newline'
```

Which characters can *start* that word is `read_token`'s business, and its first
act is the comment test — a `#` that starts a token opens a comment. So
`cat <<#c` and `cat << #c` have no delimiter either, and are the same error;
osh read `#c` as the delimiter. Once one character of the word has been read the
`#` is data, which is why `<<E#c` wants `E#c` and even `<<''#c` — where the
quotes contributed *nothing* to the word but did start it — wants `#c`. A line
continuation does not start the word (the reader deleted it), so a `#` after one
is still a comment.

**As fixed.** `read_heredoc_delim` tracks whether anything has gone into the word
yet and treats a leading `#` as a comment, consuming to the newline and stopping.
`lex_heredoc_op` then tests for "no word at all" — `delim.is_empty() && expand`,
which is exact, because only quoting can produce an empty delimiter that
consumed something and quoting clears `expand` — and on it emits the operator
token *alone*, with no `HereDoc` token after it and nothing pushed on
`pending_heredocs`. The parser already diagnoses a redirection operator with no
target, so it now says exactly what bash says, in every construct, with no new
code on that side.

Leaving the bare operator visible is also what
TD-OILS-AN-ALIAS-SPLICED-HERE-DOCUMENT-TAKES-NO-DELIMITER-FROM-THE-CALLING-LINE
needs: an alias value ending at `<<` now ends in a `Tok::Op(Op::DLess)` that the
alias pass can recognise and complete from the calling text. The stopgap that
recognition previously needed — `Lexer::dangling_delim`, `AliasBodyToks`, and
`expand_aliases_inner`'s `out.heredocs.pop()` — is gone with it.

**Pinned by** the corpus case
`a-here-document-operator-with-no-delimiter-word-is-a-grammar-error.sh` (without
the fix osh does not even terminate on it) and the unit test
`a_here_document_operator_with_no_delimiter_word_is_a_grammar_error`.

**Left behind:**
TD-OILS-A-HERE-DOCUMENT-DELIMITER-OF-A-LONE-TRAILING-BACKSLASH.
