# `for` and `select` ask the reader for one token more than any other construct,
# and it shows in the line a diagnostic carries.
#
# bash's rule is
#
#     FOR WORD newline_list IN word_list list_terminator newline_list DO …
#
# and `list_terminator` is `'\n' | ';' | yacc_EOF` (parse.y:517) — the
# end-of-file token is a terminator in its own right. So when the input runs out
# right after the word list, the EOF the last word's scan produced is *consumed*
# as the terminator, and the `newline_list` behind it still has to look ahead.
# That request finds the buffer used up, enters `shell_getc`'s fetch block and
# pays `line_number++` (parse.y:2361) a second time. Every other compound —
# `while`, `until`, `case`, `{ … }`, a function body — reaches its end of input
# through a rule that cannot accept one, so it asks once and stops.
#
# Two limits follow, and both are checked below. Only the `IN` forms carry a
# `list_terminator`, so a `for` with no list is charged once like anything else;
# and a real `;` or newline takes the terminator's place, leaving the end of
# file still to be fetched, which is again a single request.
#
# Every probe here ends its input, which would otherwise end this file — so the
# inputs are separate files run with `.`. See
# a-continuation-at-the-end-of-input-still-costs-the-reader-a-line.sh for the
# fetch accounting these numbers sit on top of.
#
# Verified against bash 5.2.37.

mk() { printf '%s' "$2" > "$1"; }

echo "=== the reference shape: a header with no list_terminator"
mk a1 'while true\
'
. ./a1; echo "rc=$?"
mk a2 'while true\
\
'
. ./a2; echo "rc=$?"
# The word ended before the run, so the deletion and the request are separate.
mk a3 'while true  \
'
. ./a3; echo "rc=$?"

echo "=== the same tail after a word list is one line further along"
mk b1 'for i in a\
'
. ./b1; echo "rc=$?"
mk b2 'for i in a\
\
'
. ./b2; echo "rc=$?"
mk b3 'for i in a b\
'
. ./b3; echo "rc=$?"
mk b4 'for i in\
'
. ./b4; echo "rc=$?"
mk b5 'for i in a  \
'
. ./b5; echo "rc=$?"
# `do` with no separator in front of it is swallowed by the word list, which is
# why this is an end of file rather than a complaint about a missing `do`.
mk b6 'for i in a do\
'
. ./b6; echo "rc=$?"

echo "=== select is the same rule"
mk c1 'select v in a\
'
. ./c1; echo "rc=$?"
mk c2 'select v in\
'
. ./c2; echo "rc=$?"

echo "=== it is the rule that asks twice, not the depth it reduces at"
mk d1 'if true; then for i in a\
'
. ./d1; echo "rc=$?"
mk d2 'for j in x; do for i in a\
'
. ./d2; echo "rc=$?"

echo "=== a real terminator takes the end-of-file token's place"
mk e1 'for i in a;\
'
. ./e1; echo "rc=$?"
mk e2 'for i in a ;\
'
. ./e2; echo "rc=$?"
mk e3 'for i in a
'
. ./e3; echo "rc=$?"
mk e4 'for i in a'
. ./e4; echo "rc=$?"

echo "=== no in, no word list, no list_terminator"
mk f1 'for i\
'
. ./f1; echo "rc=$?"
mk f2 'for i\
\
'
. ./f2; echo "rc=$?"
mk f3 'select v\
'
. ./f3; echo "rc=$?"
mk f4 'for i;\
'
. ./f4; echo "rc=$?"
