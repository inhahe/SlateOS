# `((` is an arithmetic command only where a reserved word would be
# acceptable. `parse_dparen` (parse.y:4484) gates the arithmetic reading on
# `reserved_word_acceptable (last_read_token)` and returns -2 otherwise, which
# makes `read_token` break and hand back a plain `(`.
#
# `last_read_token` is assigned in exactly one place, `yylex` (parse.y:2904).
# A conditional never goes through it: `parse_cond_command` runs from *inside*
# a single `read_token` call (parse.y:3399), and `cond_term` asks `read_token`
# for its tokens directly (parse.y:4625). So for the whole length of a
# `[[ … ]]` the last token bash remembers is `COND_START`, which is not on the
# acceptable list — and no paren in there is arithmetic, however deep.
#
# The visible consequence is that `(( … ))` inside a conditional is two nested
# groups around a *word*, so `[[ (( 0 )) ]]` is true (the non-empty string
# `0`) where `(( 0 ))` alone would be false. That is the discriminator: a
# result of 0 alone could be a coincidence.
#
# The unterminated forms have to end the input, so they live in their own
# files run with `.`.
#
# Verified against bash 5.2.37.

mk() { printf '%s\n' "$2" > "$1"; }

echo "=== nested groups, at every depth"
[[ ( a ) ]]; echo "rc=$?"
[[ (( a )) ]]; echo "rc=$?"
[[ ((( a ))) ]]; echo "rc=$?"
[[ (((( a )))) ]]; echo "rc=$?"
[[ ((((( a ))))) ]]; echo "rc=$?"
[[ ((( "" ))) ]]; echo "rc=$?"

echo "=== so the operand is a string, not an expression"
[[ (( 0 )) ]]; echo "rc=$?"
[[ (( "" )) ]]; echo "rc=$?"
[[ (( 1 )) ]]; echo "rc=$?"
[[ (( -n x )) ]]; echo "rc=$?"
[[ (( a == a )) ]]; echo "rc=$?"
[[ (( a == b )) ]]; echo "rc=$?"

echo "=== and the groups still bind the operators"
[[ ((( 0 ))) || (( 1 )) ]]; echo "rc=$?"
[[ (( "" )) || (( x )) ]]; echo "rc=$?"
[[ (( "" )) && (( x )) ]]; echo "rc=$?"
[[ ! (( "" )) ]]; echo "rc=$?"
[[ ( ( -n x ) && ( -z "" ) ) ]]; echo "rc=$?"

echo "=== a substitution's body is its own parse, so its (( is arithmetic"
[[ -n $( ((1)) ; echo hi ) ]]; echo "rc=$?"
echo "$(( 1 + 1 ))"
[[ $(( 2 * 3 )) == 6 ]]; echo "rc=$?"

echo "=== after the ]] a command position is back"
[[ a ]] && ((1)); echo "rc=$?"
[[ a ]]; ((0)); echo "rc=$?"

echo "=== the ordinary arithmetic command is untouched"
((1)); echo "rc=$?"
((0)); echo "rc=$?"
for ((i=0;i<2;i++)); do echo "i=$i"; done
if ((1+1==2)); then echo math; fi

echo "=== and so is a (( that stands where no command can start"
mk g1 'x=1 ((2))'
. ./g1
mk g2 'echo ((1))'
. ./g2

echo "=== an unfinished conditional group names the token it was handed"
mk h1 '[[ (('
. ./h1
mk h2 '[[ ((('
. ./h2
mk h3 '[[ (( a'
. ./h3
mk h4 '[[ (( a ))'
. ./h4
