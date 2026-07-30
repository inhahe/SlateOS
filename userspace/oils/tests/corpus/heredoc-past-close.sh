# A here-document whose `<<` and whose substitution's `)` share a line has
# nowhere to put its body: the substitution's own text ends at the `)`, and the
# lines that follow are the *enclosing* input's. bash's reader works a line at a
# time, so it simply fetches those lines anyway — after warning that the
# substitution closed with here-documents still ungathered.
#
# The observable consequences, each with a case below:
#
#   * the body is found, so the substitution actually produces it;
#   * the gather happens at the `)`, not at the enclosing newline, so a
#     substitution's body is taken before any here-document the rest of the line
#     declares — whatever the textual order;
#   * the reader ends up *ahead* of the line being parsed, and a token's line
#     number is the last line the reader has fetched, so `$LINENO` and any
#     diagnostic from the rest of that line name the body's last line;
#   * the warning belongs to the reader that had to do the fetching, so a nested
#     substitution's here-document is the *inner* one's to complain about, once.
#
# `heredoc-in-cmdsub.sh` covers the ordinary shape, where the body sits inside
# the substitution because the `)` is on a later line. This file is only about
# the case where it cannot.

echo "=== the body comes from past the close paren"
x=$(cat <<EOF); echo "x=[$x]"
body
EOF

echo "=== two of them, bodies in declaration order"
x=$(cat <<A; cat <<B); echo "x=[$x]"
aaa
A
bbb
B

echo "=== the substitution's body is taken before the line's own"
x=$(cat <<A) ; cat <<B
aaa
A
bbb
B
echo "x=[$x]"

echo "=== …even when the line's own here-document is declared first"
# The `<<B` is textually to the left, but the substitution is gathered at its
# `)` while B waits for the newline, so A gets the first pair of lines.
echo "arg=[$(cat <<A)]" <<B
aaa
A
bbb
B

echo "=== a here-document of an earlier command still waits its turn"
# `<<B` belongs to a command that has already been parsed *and run* by the time
# the substitution is expanded, and it is still A that gets the first lines: the
# gather order is the reader's, and the reader reached the `)` first.
cat <<B ; x=$(cat <<A); echo "x=[$x]"
aaa
A
bbb
B

echo "=== two substitutions each take their own lines"
x=$(cat <<A); y=$(cat <<C); echo "x=[$x] y=[$y]"
aaa
A
ccc
C
x=$(cat <<A) $(cat <<C); echo "x=[$x]"
aaa
A
ccc
C

echo "=== the rest of the substitution runs around the fetched body"
y=$(echo pre; cat <<EOF; echo post); echo "y=[$y]"
mid
EOF

echo "=== a subshell inside the substitution changes nothing"
x=$( ( cat <<A ) ); echo "x=[$x]"
aaa
A

echo "=== a nested substitution warns once, and it is the inner one"
x=$(echo $(cat <<A) tail); echo "x=[$x]"
aaa
A

echo "=== the reader is left ahead, so the rest of the line is stamped past it"
# `$LINENO` on the substitution's own line reports the body's last line, and so
# does the next line — which is one past it, not one past the substitution.
x=$(cat <<EOF); echo "line=$LINENO x=[$x]"
body
EOF
echo "next=$LINENO"
# The same when the substitution is alone on its line.
x=$(cat <<EOF)
body
EOF
echo "after=$LINENO x=[$x]"
# A diagnostic from the rest of the line is stamped the same way.
x=$(cat <<A); nosuchcmd1
aaa
A
echo "notfound rc=$?"

echo "=== a body that runs out warns twice, reader first"
# The input has to end for that, so these are `eval`s.
( eval 'x=$(cat <<EOF); echo "x=[$x]"' ) 2>&1
echo "eof rc=$?"
( eval 'echo one
x=$(cat <<EOF); echo "x=[$x]"' ) 2>&1
echo "eof-after-line rc=$?"
# The line's own here-document is the one left unterminated, not the
# substitution's — so both warnings appear, about different delimiters.
( eval 'echo "arg=[$(cat <<A)]" <<B
aaa
A
bbb' ) 2>&1
echo "eof-outer rc=$?"

echo done
