# A `<<` written inside an alias value is a real here-document — but its body is
# not in the alias value, and it is not on the line the alias word stands on
# either. It is the *next physical line of the file*, and the reason is that
# bash reads the two through different doors.
#
# Expanding an alias is `push_string` (parse.y:2694): the replacement becomes the
# current `shell_input_line`, the line it displaced is stacked, and `shell_getc`
# hands the replacement's characters out until it runs dry. So the `<<` is read
# by the same reader that will reach the calling line's newline, and it goes on
# the same `redir_stack` as a `<<` written on the line itself — the operator is
# in every way ordinary.
#
# The body is not read by that reader at all. `gather_here_documents` calls
# `make_here_document` (make_cmd.c:590), which calls `read_secondary_line`, which
# calls `read_a_line` (parse.y:2080) — and `read_a_line` takes its characters
# from `yy_getc()`, i.e. `bash_input.getter`, the underlying *file*. Nothing on
# that path consults `shell_input_line`, so the pushed replacement is invisible
# to it. Three consequences, all visible below:
#
#   * the body is the next line of the file after the one the alias word is on;
#   * a body written *inside* the alias value is not a body — the reader gets to
#     it later, through the ordinary door, and runs it as commands;
#   * an alias `<<` and a `<<` of the calling line share one moving cursor, in
#     `redir_stack` (declaration) order, so the earlier one takes the earlier
#     lines whichever text it was written in.
#
# The gather also moves `line_number`: `make_here_document` bumps it once per
# line `read_a_line` hands over, the delimiter's included. It fires the moment
# the reader sees a newline — which is a newline *in the alias value* when the
# value has one, so everything read after that point (the rest of the value, and
# then the rest of the calling line) is numbered from where the gather stopped.
# With no newline in the value the gather waits for the calling line's own, and
# the whole line keeps its number.

shopt -s expand_aliases

echo "=== the body is the line after the alias word, not anything in the value"
alias A='cat <<E'
A
plain body
E

echo "=== a body written in the value is read back as commands"
alias V='cat <<E
echo not-a-body'
V
real body
E

echo "=== …and those commands are numbered from where the gather stopped"
alias L='cat <<E
echo "value tail at $LINENO"'
L; echo "line tail at $LINENO"
gathered
E
echo "next line at $LINENO"

echo "=== with no newline in the value the whole line keeps its number"
alias N='cat <<E; echo "value tail at $LINENO"'
N; echo "line tail at $LINENO"
gathered
E

echo "=== <<- strips tabs, a quoted delimiter stops expansion"
alias D='cat <<-E'
D
	tabbed away
	E
alias Q='cat <<"E"'
Q
literal $HOME
E
alias U='cat <<E'
U
expanded [$0]
E

echo "=== two in one value are gathered in declaration order"
alias T='cat <<A; cat <<B'
T
first
A
second
B

echo "=== one cursor: a calling-line << before an alias one takes the earlier lines"
alias S='cat <<E'
cat <<F; S
f body
F
e body
E

echo "=== and after it"
S; cat <<G
e body 2
E
g body
G

echo "=== an alias whose value is another alias"
alias I='S'
I
via two aliases
E

echo "=== a pipeline, a compound, a function, a substitution"
S | tr a-z A-Z
piped
E
if true; then
S
in an if
E
fi
for x in 1 2; do
S
in a loop
E
done
f() {
S
in a function
E
}
f
x=$(S
from a substitution
E
)
echo "[$x]"

echo "=== a value that opens a compound leaves it open for the calling text"
alias O='{ cat <<E'
O
inside the brace
E
}

echo "=== a body need not lex as commands"
S
it's fine "and so is this
E

echo "=== redefining the alias between two uses re-reads the tail"
alias R='cat <<E'
R
for E
E
alias R='cat <<F'
R
for F
F

echo "=== unalias and shopt -u leave the word alone"
S
kept alive
E
unalias I
shopt -u expand_aliases

echo "=== last: a delimiter that never arrives warns and takes the rest"
shopt -s expand_aliases
alias Z='cat <<NEVER'
Z
tail one
tail two
