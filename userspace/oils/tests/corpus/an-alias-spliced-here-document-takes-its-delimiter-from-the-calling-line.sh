# An alias value may end *at* a `<<`, with the delimiter nowhere in it. bash
# reads the delimiter anyway, and it comes off the calling line.
#
# The reason is that one reader reads both texts. Expanding an alias is
# `push_string` (parse.y:2694): the replacement becomes the current
# `shell_input_line` and the line it displaced is stacked *with the reader's
# index just past the alias word*. When the replacement runs dry `pop_string`
# restores that line at that index, and the scan simply continues — so `<<` and
# the `E` after the alias word are one operator-and-delimiter pair, written in
# two different texts and read as one.
#
# Three things follow, all visible below:
#
#   * the delimiter is whatever word stands after the alias word, with its
#     quoting: `B "E"` does not expand the body, `B E$x` wants the literal
#     `E$x` (delimiters are never expanded), and a `\<newline>` inside it was
#     deleted before the scan ever ran;
#   * the pop chain is as deep as the push chain — `alias A='B'` over
#     `alias B='cat <<'` takes the delimiter from the line that called `A`, and
#     `alias A='B E'` takes it from A's own value;
#   * when no *word* stands there the redirection has no target, and that is a
#     grammar error at whatever token does — bash's `<<` takes a WORD and
#     nothing else. See
#     a-here-document-operator-with-no-delimiter-word-is-a-grammar-error.sh.
#
# The *body* still comes from the real input, one line at a time from the line
# after the alias word's, on the same moving cursor as every other
# here-document on that line. See
# an-alias-spliced-here-document-takes-its-body-from-the-real-input.sh.

shopt -s expand_aliases
alias B='cat <<'

echo "=== the word after the alias word is the delimiter"
B E
body one
E

echo "=== with its quoting"
B "E"
literal $HOME
E
B \E
also literal $HOME
E
x=zz
B E$x
expanded [$0]
E$x

echo "=== and with its continuations already deleted"
B E\
X
joined delimiter
EX

echo "=== the rest of the calling line is still the calling line"
B E; echo "tail at $LINENO"
gathered
E
echo "next at $LINENO"

echo "=== <<- from the value strips tabs off the calling line's body"
alias D='cat <<-'
D E
	tabbed away
	E

echo "=== the pop chain is as deep as the push chain"
alias A='B'
A E
through two aliases
E
alias V='B E'
V
delimiter from the outer value
E

echo "=== one cursor, in declaration order, across both texts"
cat <<F; B E
f body
F
e body
E
B E; cat <<G
e body 2
E
g body
G

echo "=== two dangling operators on one line"
B E; B F
for-e
E
for-f
F

echo "=== inside every construct that reads a command"
if true; then
B E
in an if
E
fi
for i in 1 2; do
B E
in a loop
E
done
f() {
B E
in a function
E
}
f
x=$(B E
from a substitution
E
)
echo "[$x]"
B E | tr a-z A-Z
piped
E

echo "=== the delimiter word is not itself alias-expanded"
alias W='NOPE'
B W
b
W

echo "=== a separator in the value ends the chain there"
# The reader gets its answer *inside* the value, so it never pops out to the
# calling line at all: `<<` has no target and that is the grammar error, at the
# `;` that stands where the word should — not at anything on the calling line.
alias P='B ; B'
( eval 'P E
one
E'; echo "rc=$?" ) 2>&1
echo "still here"

echo "=== redefining the alias between two uses"
alias R='cat <<'
R E
for E
E
alias R='cat <<-'
R F
	for F
	F

# Not probed: `alias S='cat << '` — a value ending in a *blank* — where bash
# expands the delimiter word as an alias (`PST_ALEXPNEXT`) and osh does not.
# See TD-OILS-AN-ALIAS-SPLICED-OPERATOR-DOES-NOT-EXTEND-INTO-THE-CALLING-LINE.
#
# Also not probed: `alias A='B #c'` — a value whose *comment* stands where the
# delimiter would. Both shells make it a grammar error, but bash's comment runs
# on through the pop and swallows the rest of the calling line, so it names the
# newline where osh names the first word.
# See TD-OILS-A-COMMENT-IN-AN-ALIAS-VALUE-DOES-NOT-EAT-THE-CALLING-LINE.

echo "=== last: a delimiter that never arrives warns and takes the rest"
B NEVER
tail one
tail two
