# `declare -f` has to print a here-document back as a here-document: the body
# lives on the lines after the command, not in the command itself. osh used to
# re-emit every here-doc as a here-string, which silently broke as soon as the
# body had more than one line.
#
# Note: bash's own printer drops the `;` separator from the statement after a
# here-doc when the here-doc is the *first* statement of a block, so every
# function below starts with an ordinary statement (see known-issues.md,
# TD-OILS-DECLAREF-QUIRKS).
echo "=== a plain here-doc, body and delimiter on their own lines"
h() {
	echo start
	cat <<EOF
line one
line two
EOF
	echo after
}
declare -f h

echo "=== every quoted spelling of the delimiter prints as '…'"
q() {
	echo start
	cat <<'A'
raw $x
A
	cat <<"B"
raw $x
B
	cat <<\C
raw $x
C
}
declare -f q

echo "=== <<- prints with its body already stripped of leading tabs"
t() {
	echo start
	cat <<-D
		indented
		lines
	D
}
declare -f t

echo "=== a here-doc alongside another redirection, and two on one command"
m() {
	echo start
	cat <<E >/dev/null
one
E
	cat <<F1 <<F2
first
F1
second
F2
}
declare -f m

echo "=== an empty body, and a body that expands"
e() {
	x=world
	cat <<G
G
	cat <<H
hello $x
\$literal
H
}
declare -f e

echo "=== the printed form re-parses to the same function"
eval "$(declare -f h)"
h
eval "$(declare -f t)"
t
eval "$(declare -f e)"
e

echo "=== BASH_COMMAND carries the here-doc the same way"
trap 'echo "  BC=[$BASH_COMMAND]"' DEBUG
cat <<Z >/dev/null
traced
Z
trap - DEBUG
