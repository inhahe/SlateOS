# `param_expand`'s `case LPAREN` extracts the whole `$(( … ))` before it expands
# a byte of it, and it does so through `extract_command_subst` (subst.c:10570-10575).
# For a body that starts `(` — which is exactly what `$((` is — that call is
#
#     return (extract_delimited_string (string, sindex, "$(", "(", ")", xflags|SX_COMMAND));
#                                                                              (subst.c:1284-1286)
#
# a *paren count*, so the arithmetic's own extent can never fail. What the count
# does not do is step over a nested `$( … )`: `SX_COMMAND` sends that one to
# `extract_command_subst` again — a real parse (`xparse_dolparen`) — and it is
# handed the **whole enclosing string** (subst.c:1431-1437), not the expression.
# So a body that will not parse is reported with the *string's* remainder after
# the `$(`, running out through the arithmetic's own `))` and past it.
#
# The ending is not the `${ … }`'s. Nothing here was asked to close but the
# parens, and they closed, so there is no `bad substitution`: the read simply
# swallowed the rest of the string (see `a-failed-extent-parse-consumes-the-rest-
# of-the-string.sh`) and the expansion keeps what it had up to the `$((`.
#
# Only reachable under `no_longjmp_on_fatal_error` — a prompt expansion — because
# otherwise the failed parse abandons the enclosing command outright. Written
# straight into a script, `echo A$((1+$(fi)))B` is a parse-time syntax error.
#
# Verified against bash 5.2.37.

echo "=== the diagnostic quotes the string's remainder, not the expression's"
a='A$((1+$(fi)))B';            printf '1 [%s]\n' "${a@P}"
b='A$((1+$(fi)))B$(echo Z)C';  printf '2 [%s]\n' "${b@P}"
c='A$(($((1+$(fi)))))B';       printf '3 [%s]\n' "${c@P}"
d='$((0+$(fi)))';              printf '4 [%s]\n' "${d@P}"

echo "=== and the rest of the string is consumed, not expanded"
y=Y
e='A$((1+$(fi)))B${y}C';       printf '5 [%s]\n' "${e@P}"
f='A${y}B$((1+$(fi)))C${y}D';  printf '6 [%s]\n' "${f@P}"

echo "=== nothing in the arithmetic runs, not even a substitution before the bad one"
g='A$(($(echo ran >&2)+$(fi)))B'; printf '7 [%s]\n' "${g@P}"

echo "=== a read that closes changes nothing at all"
h='A$((1+$(echo 2)))B';        printf '8 [%s]\n' "${h@P}"
i='A$((1+$(echo 2)+$(echo 3)))B'; printf '9 [%s]\n' "${i@P}"

echo "=== the arithmetic's own extent cannot fail, so an unclosed operand is just bad arithmetic"
j='A$((1+))B';                 printf '10 [%s]\n' "${j@P}"; echo "10 rc=$?"
# A backquote is `string_extract (string, &si, "\`", …)` (subst.c:1886), a byte
# hunt with no parse, so the count steps over it and the body is read only when
# the expression is expanded — against the expression's own text.
k='A$((1+`fi`))B';             printf '11 [%s]\n' "${k@P}"

echo "=== under a brace it is the brace's scan that reads it, and the brace's ending"
unset z
l='A${z:-$((1+$(fi)))}B';      printf '12 [%s]\n' "${l@P}"
m='A${z:-"p$((1+$(fi)))q"}B';  printf '13 [%s]\n' "${m@P}"
n='A${z@$((1+$(fi)))}B';       printf '14 [%s]\n' "${n@P}"

echo "=== the body read is a whole parse, so it carries its here-documents"
o='A$((1+$(cat <<E
hi
E
)))B';                         printf '15 [%s]\n' "${o@P}"
# A body that *fails* to parse with a newline in it is left out on purpose. The
# read stops on the line the reader reached rather than at the end of the string
# (`a-failed-extent-read-stops-on-the-line-the-reader-reached.sh`), which here
# leaves the paren count to carry on from there and close early — bash gives
# `[A)B]` for `A$((1+$(fi⏎echo x⏎)))B` where the one-line shape gives `[A]`, and
# reports a second time from the fallback child. osh does not reproduce that yet;
# see `TD-OILS-AN-ARITHMETIC-EXTENT-CARRIES-ON-COUNTING-AFTER-A-FAILED-READ` in
# known-issues.md.

echo "=== a single-quoted run inside the arithmetic is still parsed, being a real parse"
q='A$((1+$(echo "x" ; fi)))B'; printf '17 [%s]\n' "${q@P}"

echo "=== done"
