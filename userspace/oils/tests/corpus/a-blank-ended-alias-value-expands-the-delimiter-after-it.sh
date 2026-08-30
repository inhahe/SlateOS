# A here-document delimiter is an ordinary word to the reader, so the flag an
# alias value ending in a blank leaves behind expands it like any other word.
#
# `<<`'s target is a WORD in bash's grammar (`redirection: '<' '<' WORD`), read
# by `read_token_word`, which runs the alias check on whatever it just built
# whenever `quoted == 0` (parse.y:5266). Only the *position* differs from a
# command word: reading the `<` clears `PST_ALEXPNEXT` (parse.y:3511), and
# nothing about a redirection target sets it — so the delimiter is a candidate
# exactly when the pop of a blank-ended value sets the flag again between the
# operator and it.

shopt -s expand_aliases
alias W='E9'

echo "=== the delimiter looked for is the value, not the word written"
alias S='cat << '
S W
one
E9

echo "=== <<- the same"
alias T='cat <<- '
T W
	two
	E9

echo "=== quoting inhibits the lookup"
# And a quoted delimiter is exactly the non-expanding one, so the two questions
# have one answer.
S 'W'
three
W

echo "=== without the trailing blank there is no flag to carry over"
alias U='cat <<'
U W
four
W

echo "=== the flag is spent on the delimiter and does not reach past it"
# The second `W` is an operand, so `cat` reads the *file* `W`; had the flag
# survived it would have become `E9`, and there is no such file.
printf 'operand\n' > W
S W W
five
E9
echo "rc=$?"

echo "=== a value of several words leaves the rest on the line"
# `cat << X Y` — `X` is the delimiter and `Y` an operand, so the here-document
# runs off the end of the script looking for `X` while `cat` prints the file.
printf 'other\n' > Y
alias V='X Y'
alias S2='cat << '
S2 V
six
X Y
