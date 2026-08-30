# bash has two answers for "the input ended inside `[[ … ]]`", and it picks by
# whether the conditional grammar still wanted something.
#
# If it did, `cond_term` asks `read_token` for the token it is waiting on and is
# handed the end-of-file one. That is a token like any other and it is rejected
# where it stands — `error_token_from_token` can spell this one, so the message
# carries `EOF` in the slot a newline or a stray operator would occupy. Only a
# conditional that is *complete* and merely unclosed gets `unexpected EOF while
# looking for` — and that one is reported on the line the reader gave up on,
# not on the end-of-file line.
#
# None of the end-of-file forms carries a `syntax error near` line. The fetch
# that discovered the end left `shell_input_line` empty, so
# `report_syntax_error` falls past both of its `near` branches to the
# end-of-file one (parse.y:6273).
#
# A group adds the `)` it never reached at the line `cond_term` captured *after*
# reading its `(` (parse.y:4648). `(` is a `shellmeta`, so `read_token` peeks
# the next character with `shell_getc (1)` — continuation removal on — to see
# whether this is `((`, and that peek deletes a `\<newline>` written flush
# against the paren, `line_number++` and all. So the same group is numbered a
# line further down when its `(` ends the line than when anything follows it.
#
# The input has to genuinely run out, which a file cannot do on its own: bash
# writes back the newline a last line did not come with. So each probe ends on a
# `\<newline>`, which the reader deletes and then finds nothing behind. They are
# separate files run with `.` so that ending the input does not end this script.
#
# Verified against bash 5.2.37.

mk() { printf '%s\n' "echo 1" "$2" > "$1"; }

echo "=== the three operand slots each name the token they were handed"
mk a1 '[[ a\'
. ./a1
mk a2 '[[ a ==\'
. ./a2
mk a3 '[[ -f\'
. ./a3
mk a4 '[[ !\'
. ./a4
mk a5 '[[ a =~\'
. ./a5
mk a6 '[[ a -eq\'
. ./a6
mk a7 '[[ a &&\'
. ./a7
mk a8 '[[ a ||\'
. ./a8

echo "=== a complete expression wants only the closer, and says so where it stopped"
mk b1 '[[ a == b\'
. ./b1
mk b2 '[[ -n x\'
. ./b2
mk b3 '[[ ( a )\'
. ./b3

echo "=== a group's own line is the one its ( was read on, peek and all"
mk c1 '[[ (\'
. ./c1
mk c2 '[[ (   \'
. ./c2
mk c3 '[[ ( (\'
. ./c3
mk c4 '[[ ((\'
. ./c4
mk c5 '[[ ( ( (\'
. ./c5
mk c6 '[[ ( a\'
. ./c6
mk c7 '[[ ( ( a\'
. ./c7
mk c8 '[[ ( -f\'
. ./c8
mk c9 '[[ ( a == b\'
. ./c9

echo "=== and without the continuation, where a newline is the token instead"
mk d1 '[[ a'
. ./d1
mk d2 '[[ a =='
. ./d2
mk d3 '[[ -f'
. ./d3
mk d4 '[[ ('
. ./d4
mk d5 '[[ ( ('
. ./d5
mk d6 '[[ ( a'
. ./d6
