# `heredoc-past-close.sh` establishes that a here-document whose `<<` and whose
# `)` share a line is fetched *at the `)`*, from the lines the enclosing input
# has not reached yet. This file is about what happens when that is true at two
# levels of nesting at once, which is the case that shows there is only ever
# **one reader**.
#
# bash reads a line at a time into `shell_input_line`, and every level of
# `$( … )` draws from that same reader — `parse_comsub` is the ordinary parser
# running on the ordinary input. So the fetching is strictly in the order the
# scan meets the closing parens: the *inner* `)` comes first, so the inner
# substitution's bodies are taken first, and by the time the outer `)` is
# reached the reader has already moved past them.
#
# That is why the two warnings do not share a line. The first names the line the
# reader was on when the inner `)` was met; the second names the line the inner
# gather left it on. The `here-document … delimited by end-of-file` that follows
# an outer body running out is stamped the same way, from the same reader.
#
# The consequence for the *text* is that the inner delimiter is looked for
# first, in lines that a naive reading would have given to the outer one: in the
# first case below `<<B` swallows `aaa` and the line `A` along with it, and the
# outer `<<A` is left with the line after them.
#
# See known-issues TD-OILS-CMDSUB-HEREDOC-NESTED-GATHER-ORDER.

echo "=== the inner close fetches first, so the outer warning is a line later"
x=$(cat <<A; echo $(cat <<B) mid); echo "x=[$x]"
aaa
A
bbb
B
A

echo "=== …and the inner delimiter is the one looked for whatever the order"
# `<<A` is written first and `<<B` still gets the first pair of lines.
x=$(cat <<A; echo $(cat <<B) mid); echo "x=[$x]"
bbb
B
aaa
A

echo "=== two at the inner level are one warning, plural, then the outer"
x=$(cat <<A; echo $(cat <<B; cat <<C) mid); echo "x=[$x]"
1
B
2
C
3
A

echo "=== three levels deep is still one reader"
x=$(echo $(echo $(cat <<C) i2) i1); echo "x=[$x]"
ccc
C

echo "=== two inner substitutions side by side each take their own lines"
x=$(echo $(cat <<B) $(cat <<C) end); echo "x=[$x]"
bbb
B
ccc
C

echo "=== a process substitution nested inside is the same nesting"
x=$( echo $(cat <(cat <<B)) z ); echo "x=[$x]"
bbb
B

echo "=== a case inside the inner substitution does not confuse the close"
x=$(cat <<A; echo $(case q in q) cat <<B;; esac) m); echo "x=[$x]"
bbb
B
aaa
A

echo "=== an inner body delimited in place leaves nothing to fetch"
# The `)` is on a later line, so B's body is inside the substitution's own text
# and no warning is raised at either level.
x=$(echo $(cat <<B
bbb
B
) mid); echo "x=[$x]"

echo "=== the reader is left past both bodies, so the rest of the line follows"
x=$(cat <<A; echo $(cat <<B) m); echo "line=$LINENO x=[$x]"
bbb
B
aaa
A
echo "next=$LINENO"

echo "=== a later substitution on a later line carries on from there"
x=$(cat <<A; echo $(cat <<B) m); y=$(cat <<D); echo "x=[$x] y=[$y]"
bbb
B
aaa
A
ddd
D

echo "=== when the input runs out, the end-of-file warning uses the same line"
# The input has to end for that, so these are `eval`s.
( eval 'x=$(cat <<A; echo $(cat <<B) mid); echo "x=[$x]"
aaa
A
bbb
B' ) 2>&1
echo "eof rc=$?"
( eval 'x=$(cat <<A; echo $(cat <<B) mid); echo "x=[$x]"' ) 2>&1
echo "eof-both rc=$?"

echo done
