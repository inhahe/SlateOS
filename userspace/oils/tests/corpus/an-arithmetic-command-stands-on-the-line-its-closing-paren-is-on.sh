# `(( … ))` has a line of its own, and it is the line its **closing** `))` sits
# on — not the line it starts on, and not whatever line the enclosing construct
# had reached.
#
# `make_arith_command` (make_cmd.c:438) stamps it at the reduction of
# `arith_command: ARITH_CMD`:
#
#	  temp->flags = 0;
#	  temp->line = line_number;
#	  temp->exp = exp;
#
# and `ARITH_CMD` is a *single* token, scanned whole by `parse_arith_cmd` with
# `parse_matched_pair (0, '(', ')', &ttoklen, P_ARITH)` (parse.y:4519) — so by
# the time the rule reduces, the reader has been carried to wherever the `))`
# was found. `execute_arith_command` (execute_cmd.c:3795) then installs it
# around the whole evaluation and puts the enclosing line back afterwards:
#
#	  save_line_number = line_number;
#	  this_command_name = "((";	/* )) */
#	  SET_LINE_NUMBER (arith_command->line);
#
# Both halves show: `$LINENO` read inside the expression, and the line a
# diagnostic raised by the evaluation is blamed on.
exec 2>&1
echo "--- \$LINENO inside the expression, per enclosing construct"
{
echo one
(( x1 = LINENO ))
}
echo "x1=$x1"
for i in 1; do
(( x2 = LINENO ))
done
echo "x2=$x2"
f() {
(( x3 = LINENO ))
}
f
echo "x3=$x3"
g() { echo x
(( x4 = LINENO ))
}
g
echo "x4=$x4"
while (( x5 = LINENO )); do break; done
echo "x5=$x5"
case c in c) (( x6 = LINENO ));; esac
echo "x6=$x6"
echo "--- a multi-line one is stamped at its close, not its open"
(( x7 =
   LINENO ))
echo "x7=$x7"
(( x8 = \
   LINENO ))
echo "x8=$x8"
echo "--- and the enclosing line comes back afterwards"
echo "after=$LINENO"
echo "--- a diagnostic out of the evaluation lands on the same line"
declare -n c1=c2
declare -n c2=c1
{
echo two
(( c1 = 1 ))
}
for i in 1; do
(( c1 = 2 ))
done
h() {
(( c1 = 3 ))
}
h
if true; then (( c1 = 4 ))
fi
(( c1 =
   5 ))
echo "--- an expansion is not a command and keeps the enclosing line"
{
echo three
echo "sub=$(( c3 = LINENO ))"
}
echo TAIL
