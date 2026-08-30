# The line after `syntax error in conditional expression …` is
# `report_syntax_error` picking a branch, and which one it picks is the
# *reader's* business, not the grammar's.
#
# A conditional failure returns `-1` from `read_token` (parse.y:3402), and
# `error_token_from_token` cannot name that, so the first branch (parse.y:6251)
# is skipped and the second is tried — `if (shell_input_line &&
# *shell_input_line)` (parse.y:6273). With text left on the line bash finds an
# offending token in it, prints `syntax error near `X'` and echoes the line.
# With the line empty it falls to the final `else` and prints a bare
# `syntax error`, with nothing under it.
#
# The only thing that empties the line is a fetch that found the end of the
# input, and a token's own reading never provokes one from a newline-closed
# string — it stops on the newline. So this shape needs a string closed with a
# backslash: the look past the offending token deletes the `\<newline>`, runs
# the buffer out, and fetches nothing.
#
# Which makes the discriminator a single space. `[[ a == b )\` stops past the
# end and loses its `near` line; `[[ a == b ) \` stops on the space, still has
# the whole line, and reports near `)` on the `[[`'s own line — echoing the
# trailing backslash along with it.
#
# `EOF_Reached` is 0 throughout: the conditional died on a token it *had*, and
# nothing ever asked for one past the close. That is why these say `syntax
# error` and not `syntax error: unexpected end of file`.
#
# The general (non-conditional) errors are the contrast: they go out through the
# *first* branch, whose `print_offending_line` is unconditional (parse.y:6262),
# so they keep both lines and simply echo nothing.
#
# Verified against bash 5.2.37.

mk() { f=$1; shift; printf '%s\n' "$@" > "$f"; }

echo "=== the offending token ran the input out: no near line, nothing echoed"
mk a1 'echo 1' '[[ a == b )\'
. ./a1
mk a2 'echo 1' '[[ -z x y\'
. ./a2
mk a3 'echo 1' '[[ a b\'
. ./a3
mk a4 'echo 1' '[[ ( a ;\'
. ./a4
mk a5 'echo 1' '[[ a == b ;\'
. ./a5

echo "=== one space and the read stops short of it, so the line is still there"
mk b1 'echo 1' '[[ a == b ) \'
. ./b1
mk b2 'echo 1' '[[ a == b )'
. ./b2
mk b3 'echo 1' '[[ a -eq b -eq c\'
. ./b3
mk b4 'echo 1' '[[ a -eq b -eq c \'
. ./b4
mk b5 'echo 1' '[[ ( a ; \'
. ./b5
mk b6 'echo 1' '[[ a b )\' 'echo 2'
. ./b6

echo "=== the fetch found a line rather than the end, so that one is reported"
mk c1 'echo 1' '[[ a == b )\' 'echo 2'
. ./c1
mk c2 'echo 1' '[[ a == b ;\' 'echo 2'
. ./c2

echo "=== an ordinary syntax error keeps both lines and echoes nothing"
mk d1 'echo 1' ';;\'
. ./d1
mk d2 'echo 1' 'echo a; )\'
. ./d2
mk d3 'echo 1' '[[ a == b ]] ;;\'
. ./d3
mk d4 'echo 1' 'if ;\'
. ./d4

echo "=== a conditional that ran out of input is a different branch again"
mk e1 'echo 1' '[[ -f -o\'
. ./e1
mk e2 'echo 1' '[[ a ==\'
. ./e2
mk e3 'echo 1' '[[ a == b\'
. ./e3
