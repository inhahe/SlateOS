# A `(( … ))` whose two closing parentheses are not adjacent is handed back to
# the ordinary grammar as `( ( … ) )` — except in a `for` header, where there is
# no subshell to fall back to and the failed test is simply an error. That much
# is pinned by an-arithmetic-command-needs-its-closing-parentheses-adjacent.sh.
# This is about *how* the error reads, which is unlike any other syntax error
# the shell prints.
#
# `parse_dparen`'s `ARITH_FOR_EXPRS` arm returns −1 (parse.y:4478), and bison
# reads a negative token as its EOF token — while `EOF_Reached` stays clear. So
# `report_syntax_error` finds `current_token` non-zero, asks
# `error_token_from_token` to name it, and gets NULL back, because the switch has
# no branch for −1 (parse.y:6251). It therefore falls through to its *other*
# branch and reports what `error_token_from_text` slices out of the input around
# `shell_input_line_index` (parse.y:6276): `syntax error near \`X'` — no
# "unexpected token" — with the offending line echoed underneath.
#
# The index it slices around is where `parse_arith_cmd` parked it: one past the
# single character its `c = shell_getc (0)` read to test for the second `)`.
# `error_token_from_text` then steps back over trailing whitespace, back again to
# the nearest ` `, `\n`, `\t`, `;`, `|` or `&`, and returns everything from there
# up to and including that character. Which is why the answer swings so far on
# what is written after the body: a space before the stray `)` cuts the slice
# down to that one character, and no space at all drags in the tail of the
# expression.
n() { sed 's/^.*: line /line /'; }
q() { ( eval "$1"; echo "rc=$?" ) 2>&1 | n; }

echo "=== the slice stops at the delimiter before the offending character"
q 'for ((i=0;i<1;i++) ) ; do echo $i; done'
q 'for ((i=0;i<1;i++)x ; do echo $i; done'
q 'for ((i=0;i<1;i++)  ; do echo $i; done'
q 'for ((1)  ; do :; done'
q 'for (( ) ) ; do :; done'
q 'for ((;;) ) ; do :; done'
q 'for	((1) ) ; do :; done'

echo "=== the character the test read may be the end of the line"
q 'for ((1)'
q 'for ((i=0;i<1;i++)'
# A newline is read as itself here — `shell_getc (0)` does not delete a
# continuation, and does not fetch — so the reader is left at the NUL past the
# line it is on, not at the top of the next one, and both the slice and the
# echoed line are still the first line's.
q 'for ((i=0;i<1;i++)
) ; do echo $i; done'
q 'for ((i=0;i<1;i++)\
) ; do echo $i; done'

echo "=== written anywhere a for header can be"
q 'if true; then for ((1) ); fi'
q 'while :; do for ((1) ); done'
q '{ for ((1) ); }'
q 'f() { for ((1) ); }'
q 'echo a
for ((1) )'

echo "=== and only a for header: everything else falls back to subshells"
q 'select x in ((1) ) ; do :; done'
q 'echo for ((1) )'
q '((1) ); echo "rc-inner=$?"'
q 'for ((i=0;i<2;i++)); do echo "i=$i"; done'
echo "=== done"
