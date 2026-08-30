# A *string* whose last line ends on an odd run of backslashes is closed with
# another backslash rather than with a newline, and that costs the reader a line
# nothing else does.
#
# `shell_getc` writes back the newline a line did not come with — except for an
# `st_string` whose last character is an unescaped backslash, where a newline
# would be eaten as a continuation. There it writes back a *backslash* instead
# (parse.y:2567, quoted in `parser::close_last_line`), so the pair is one quoted
# literal backslash and the input ends with no newline at all.
#
# Two fetches follow, where a newline-closed input pays one. The last token's own
# scan has to discover that the token ended, finds the buffer used up and enters
# `shell_getc`'s fetch block — `line_number++` (parse.y:2361) — and then the
# parser's request for the token after it enters the same block again. bash also
# has no newline token left to hand over, so what the grammar sees is the
# end-of-file token, which is why `case x in  \` is an unexpected *end of file*
# rather than an unexpected newline.
#
# Every probe here ends its input, which would otherwise end this file — so the
# inputs are separate files run with `.`, which is bash's string reader all the
# same. An even run is the control: it closes with a newline like anything else.
#
# Verified against bash 5.2.37.

mk() { printf '%s' "$2" > "$1"; }

echo "=== the line a command is stamped with"
mk a1 'echo 1
echo $LINENO\'
. ./a1
mk a2 'echo 1
echo $LINENO\\'
. ./a2
mk a3 'echo 1
echo $LINENO\\\'
. ./a3
mk a4 'echo 1
echo $LINENO\\\\'
. ./a4
# The lookahead is the second word, which ended before the close.
mk a5 'echo 1
echo $LINENO A\'
. ./a5
mk a6 'echo 1
echo $LINENO  \'
. ./a6
# A continuation on the line before, and then the close.
mk a7 'echo 1
echo $LINENO\
\'
. ./a7

echo "=== the same line in a diagnostic"
mk b1 'echo 1
cd $LINENO\'
. ./b1; echo "rc=$?"
mk b2 'echo 1
cd $LINENO\\'
. ./b2; echo "rc=$?"

echo "=== an unterminated here-document is charged both fetches"
mk c1 'echo 1
cat <<x\'
. ./c1; echo "rc=$?"
mk c2 'echo 1
cat <<x\\'
. ./c2; echo "rc=$?"

echo "=== a compound that never closes"
mk d1 'echo 1
if true; then\'
. ./d1; echo "rc=$?"
mk d2 'echo 1
if true; then\\'
. ./d2; echo "rc=$?"
mk d3 'echo 1
{ echo hi\'
. ./d3; echo "rc=$?"
mk d4 'echo 1
for i in a\'
. ./d4; echo "rc=$?"
mk d5 'echo 1
for i\'
. ./d5; echo "rc=$?"
mk d6 'echo 1
f() {  \'
. ./d6; echo "rc=$?"

echo "=== the close makes the last word ordinary, so a reserved word is not one"
mk e1 'echo 1
case x in\'
. ./e1; echo "rc=$?"
mk e2 'echo 1
f() {\'
. ./e2; echo "rc=$?"
mk e3 'echo 1
for i  \'
. ./e3; echo "rc=$?"

echo "=== with no newline left, the grammar sees the end of file"
mk f1 'echo 1
case x in  \'
. ./f1; echo "rc=$?"
