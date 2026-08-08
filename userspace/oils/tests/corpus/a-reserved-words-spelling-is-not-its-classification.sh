# `((` is an arithmetic command only where a reserved word would be recognised,
# and *that* is decided by the previous token's classification, not its text.
#
# `parse_dparen` (parse.y:4484) gates the arithmetic reading on
# `reserved_word_acceptable (last_read_token)`. `last_read_token` holds the
# token bash's grammar was handed, and a word becomes a reserved word only
# where one was already acceptable — `CHECK_FOR_RESERVED_WORD` (parse.y:2994)
# is gated on the very same test. The rule is therefore recursive: `do` opens a
# loop body after `;` and is an argument after `echo`.
#
# So the discriminator is a word that *looks* reserved standing where it is
# not. `; do ((1))` runs an arithmetic command; `echo do ((1))` is a syntax
# error near `(`, exactly as `echo x ((1))` is — the `do` bought nothing.
#
# The other direction is `]]`, which is `COND_END` and *is* on the acceptable
# list — so `[[ a ]] ((1))` reads the `((` as arithmetic and the grammar
# complains about the expression it collected, naming `1`, where `echo ]] ((1))`
# leaves an ordinary word behind and names the `(`.
#
# `reserved_word_acceptable`'s default branch has two lookbehinds
# (parse.y:5406–5412): a WORD is acceptable when the token before *it* was
# `function` or `coproc`. That is one word deep, not a mode — `function f g`
# is already an error at `g`.
#
# The failing forms end the parse unit, so they live in their own files run
# with `.`.
#
# Verified against bash 5.2.37.

mk() { printf '%s\n' "$2" > "$1"; }

echo "=== where a reserved word may stand, the (( is arithmetic"
if ((1)); then echo if-yes; fi
while ((0)); do echo never; done; echo "while rc=$?"
until ((1)); do echo never; done; echo "until rc=$?"
{ ((1)); }; echo "brace rc=$?"
! ((1)); echo "bang rc=$?"
# `time`'s own report goes to the shell's stderr and carries a clock, so only
# the status it forwards is comparable.
( time ((1)) ) 2>/dev/null; echo "time rc=$?"
for ((i=0;i<2;i++)); do echo "i=$i"; done
echo a | ((1)); echo "pipe rc=$?"
echo a && ((1)); echo "andand rc=$?"
( ((1)) ); echo "subshell rc=$?"
((1)); echo "plain rc=$?"

echo "=== the same spellings in an argument position are ordinary words"
mk b1 'echo do ((1))'
. ./b1
mk b2 'echo done ((1))'
. ./b2
mk b3 'echo fi ((1))'
. ./b3
mk b4 'echo then ((1))'
. ./b4
mk b5 'echo esac ((1))'
. ./b5
mk b6 'echo until ((1))'
. ./b6
mk b7 'echo time ((1))'
. ./b7
mk b8 'echo ! ((1))'
. ./b8
mk b9 'echo { ((1))'
. ./b9
mk b10 'echo } ((1))'
. ./b10
mk b11 'echo if ((1))'
. ./b11
mk b12 'x=1 ((2))'
. ./b12

echo "=== ]] is COND_END where it closes a conditional, and a word elsewhere"
# Which is visible in the name the error gives: `error_token_from_token` returns
# `string_list (yylval.word_list)` for an `ARITH_CMD` (parse.y), so the token it
# names is the expression the arithmetic reading collected — verbatim, inner
# spaces and all — where a `((` that stayed two parens is named `(`.
mk c1 '[[ a ]] ((1))'
. ./c1
mk c2 'echo ]] ((1))'
. ./c2
mk c3 '[[ a ]] (( 1 + 1 ))'
. ./c3
mk c4 '[[ a ]] ((x = 1))'
. ./c4
mk c5 '[[ a ]] (( $(echo 2) ))'
. ./c5
mk c6 '[[ a ]] ((  ))'
. ./c6
[[ a ]]; ((1)); echo "separated rc=$?"

echo "=== a name is acceptable after function and after coproc"
coproc c ((1)); echo "coproc rc=$?"
wait "$c_PID" 2>/dev/null
mk d1 'function f g ((1))'
. ./d1

echo "=== and the words bash leaves off the list are still off it"
mk e1 'for i in ((1))'
. ./e1
mk e2 'select i in ((1))'
. ./e2
mk e3 'case x in ((p) echo hi;; esac'
. ./e3
