# Every command-substitution body is numbered from the line the *shell* is on
# when the substitution is expanded — never from anything in the text the body
# was written in. `command_substitute` (subst.c:6986) passes
# `pflags = (interactive && sourcelevel == 0) ? SEVAL_RESETLINE : 0`, so a
# non-interactive shell resets nothing: the child starts from the inherited
# `line_number` and pays the single `line_number--` that `parse_and_execute`
# does (evalstring.c:329), which puts the body's first line back on the line
# being expanded. That line is exactly the one `$LINENO` reports, so every row
# below prints the outer `$LINENO` beside the inner one and the two agree
# wherever the body's first line is the one carrying the command.
#
# For a one-line command the base also happens to be the closing delimiter's
# line, which is why the rule is invisible on most shapes. The shapes that tell
# them apart are the ones where the word is *not* on the line the command is
# numbered by: an element of a compound assignment, and a here-document body.
#
# The two spellings differ only in what text is counted. A `$( … )` body is
# re-printed before it is re-read, so its blank and continuation lines are gone
# and its line 1 is the first command; a backquote body is echoed verbatim, so
# the source's own blank lines still count.
#
# Verified against bash 5.2.37.

echo "=== the base is the outer \$LINENO, in every spelling"
echo "a $LINENO $(echo $LINENO)"
echo "b $LINENO `echo $LINENO`"
echo "c $LINENO $(( `echo $LINENO` )) $(( $(echo $LINENO) ))"

echo "=== a body written across lines counts from that same base"
# The `$( … )` re-print has no leading blank line, so its command is line 1 of
# the body and lands back on the base; the backquote keeps one, so it is line 2.
echo "d $LINENO $(
echo $LINENO)"
echo "e $LINENO `
echo $LINENO`"
echo "f $LINENO $(echo $LINENO

echo $LINENO)"
echo "g $LINENO `echo $LINENO

echo $LINENO`"

echo "=== an element of a compound assignment takes the assignment's line"
# The words sit on lines of their own, and none of them is the answer.
arr=(
  "h $LINENO $(echo $LINENO)"
  "i $LINENO `echo $LINENO`"
)
printf '%s\n' "${arr[@]}"
# `declare` is numbered from its *opening* line where a bare assignment is
# numbered from its closing one, so the two disagree in opposite directions.
declare -A m=(
  [k]="j $LINENO $(echo $LINENO)"
  [l]="k $LINENO `echo $LINENO`"
)
printf '%s\n' "${m[k]}" "${m[l]}"

echo "=== a here-document body takes the redirecting command's line"
cat <<E
l $LINENO $(echo $LINENO)
E
cat <<E


m $LINENO $(echo $LINENO)
n $LINENO `echo $LINENO`
E

echo "=== a diagnostic out of a body is numbered the same way"
q() { printf '%-6s ' "$1"; shift; "$@" 2>&1 | sed 's/^[^:]*: //'; }
arr2=(
  "$(nosuchcmd_a)"
  "`nosuchcmd_b`"
)
printf '%s\n' "${arr2[@]}" 2>&1
cat <<E
$(nosuchcmd_c)
E

echo "=== and a body reached from inside a function or an eval"
f() {
  echo "o $LINENO $(echo $LINENO) `echo $LINENO`"
}
f
eval 'echo "p $LINENO $(echo $LINENO) `echo $LINENO`"'

echo done
