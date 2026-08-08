# bash's `line_number` counts *fetches*, not lines that have text on them. Two
# things follow from that, and both only show up at the very end of an input:
#
#   1. Deleting a `\<newline>` is a fetch. `shell_getc` (parse.y ~2361) opens its
#      fetch block with `line_number++`, and the continuation-deleting path does
#      its own `line_number++; goto restart_read;` — so a run of continuations at
#      the end of the input walks the counter forward even though there is no
#      more text.
#   2. Asking for a token that is not there is a fetch too. The parser's request
#      for its next lookahead runs the buffer out and enters that same block, so
#      the number a diagnostic carries is typically one *past* the last line the
#      scanner actually stopped on.
#
# The two interact, and not by simple addition. An operator is read by peeking
# one character past it (to tell `>` from `>>`, `|` from `||`), and that peek is
# handed back with `shell_ungetc` — which at the start of a line stows it in
# `eol_ungetc_lookahead`, where the next `shell_getc` finds it *before* fetching
# anything. So when an operator's own peek is what deleted the last continuation
# and ran into the end of input, the parser's request for that end-of-file token
# is free, and only the deletions are charged. A word is not read that way
# (`read_token_word` stops on the character that ended it and pushes nothing
# back), so after a word both are charged.
#
# Every probe here ends its input, which would otherwise end this file — so the
# inputs are separate files run with `.`, plus a few `eval`s. Note `.` and
# `eval` are the *string* reader while this file is the *stream* reader; they
# differ only in how a last line with no newline at all is closed (see
# the-two-readers-close-an-unterminated-last-line-differently.sh), and every
# input below is written with its final newline, so the rule is the same one.
#
# Verified against bash 5.2.37.

mk() { printf '%s' "$2" > "$1"; }

echo "=== an unterminated here-document is blamed on the fetch, not the text"
# No continuation: the delimiter is on line 1 and the reader never left it, so
# the body "began" and the input ran out on the same line.
mk a1 'cat <<x'
. ./a1
# One `\<newline>` after the delimiter word: the deletion is fetch 2, and the
# parser's request for the end-of-file token is fetch 3. Both numbers are 3.
mk a2 'cat <<x\
'
. ./a2
# Each further continuation is one more fetch.
mk a3 'cat <<x\
\
'
. ./a3

echo "=== \$LINENO sees the same counter"
mk b1 'echo $LINENO'
. ./b1
# A simple command is numbered after one token of lookahead, and the lookahead
# word's terminator scan is what walks into the deleted continuation.
mk b2 'echo $LINENO\
'
. ./b2
mk b3 'echo $LINENO\
\
'
. ./b3

echo "=== and so does the line a syntax error is blamed on"
# The `>` has nothing to redirect to; the newline it finds is the offending
# token, on the line it sits on.
mk c1 'cat >
'
. ./c1; echo "rc=$?"
# With the newline deleted the error becomes an end-of-file instead — and the
# peek that reads `>` is what deleted it and found the end of input, so that end
# of file is stowed and the parser gets it back without a fetch of its own.
mk c2 'cat >\
'
. ./c2; echo "rc=$?"
mk c3 'cat >\
\
'
. ./c3; echo "rc=$?"
# A compound left open runs out of input either way; note the no-continuation
# form is already one past its text, because of the end-of-file request.
mk d1 'if true; then
'
. ./d1; echo "rc=$?"
mk d2 'if true; then\
'
. ./d2; echo "rc=$?"
# A token the grammar rejects outright moves with the reader too, and the line
# it echoes back is the empty one the reader ended up on.
mk e1 'echo )\
'
. ./e1; echo "rc=$?"

echo "=== eval reads a string, and counts the caller's lines"
eval 'echo $LINENO'
eval 'echo $LINENO\
'
eval 'cat <<x\
'
eval 'cat >\
'; echo "rc=$?"
