# An unterminated here-document is not an error: the shell accepts the body it
# managed to collect, runs the command, and *warns*. The warning is worth a case
# of its own because of the two line numbers it carries, neither of which is the
# line the `<<` operator sits on:
#
#   * the one in the prefix is where the input ran out;
#   * the one in the message is the last line read when the *body* began — the
#     operator's own line for a lone here-doc, but for a second one the line the
#     first one's body stopped on.
#
# Both are "the last line the reader had fetched", which is why a trailing
# newline adds nothing to either (the reader stops at the boundary and never asks
# for the empty line past it) while a final line without one still counts.
#
# Every case needs an input of its own that *ends*, so each one is an `eval` or a
# sourced file rather than a line of this script.

echo "=== the base shape, and what a trailing newline does not change ==="
eval 'cat <<EOF
body' 2>&1
echo "rc=$?"
eval 'cat <<EOF
body
' 2>&1
echo "trailing rc=$?"

echo "=== an empty body makes the two numbers coincide ==="
# So the prefix line is not just the message's line plus one.
eval 'echo hi
echo ho
cat <<EOF' 2>&1
echo "empty rc=$?"

echo "=== earlier lines run first, and are not counted twice ==="
eval 'echo hi
cat <<EOF
body' 2>&1
echo "after-line rc=$?"

echo "=== a second here-document is blamed on where the first one stopped ==="
eval 'cat <<A <<B
one
A
two' 2>&1
echo "second rc=$?"
# Both cut off: A stops at end of input, so B's body "begins" there too.
eval 'cat <<A <<B
one' 2>&1
echo "both rc=$?"

echo "=== a terminated here-document earlier in the input shifts nothing ==="
eval 'cat <<EOF
body
EOF
cat <<X
more' 2>&1
echo "mixed rc=$?"

echo "=== the delimiter is reported as written, minus its quotes ==="
eval 'cat <<"EOF"
body' 2>&1
echo "quoted rc=$?"
eval 'cat <<-EOF
	body' 2>&1
echo "strip rc=$?"

echo "=== the command still runs, redirections and all ==="
eval 'cat <<EOF; echo after
body' 2>&1
echo "same-line rc=$?"
# A properly terminated here-doc warns not at all.
eval 'cat <<EOF
body
EOF' 2>&1
echo "terminated rc=$?"

echo "=== the warning precedes a syntax error the same input also has ==="
# The body swallows the `}`/`done` that would have closed the construct, so the
# input ends mid-command — and the reader's warning comes out before the parser's
# complaint.
( eval 'f() { cat <<EOF
body' ) 2>&1
echo "func rc=$?"
( eval 'for i in 1 2; do cat <<EOF
b$i
done' ) 2>&1
echo "loop rc=$?"

echo "=== a script read as a file names the file, and its own lines ==="
printf 'echo hi\ncat <<EOF\nbody\n' > sub.sh
. ./sub.sh 2>&1
echo "source rc=$?"
# The last line having no newline of its own does not change the count.
printf 'echo hi\ncat <<EOF\nbody' > sub2.sh
. ./sub2.sh 2>&1
echo "source-nonl rc=$?"

echo done
